"""Synthetic regression tests for automatic, structurally gated release images."""
import importlib.util
import json
import os
from pathlib import Path
import sys
import tempfile
import unittest
from unittest.mock import patch

sys.dont_write_bytecode = True
ROOT = Path(__file__).resolve().parents[1]
spec = importlib.util.spec_from_file_location('release_image', ROOT / 'scripts/check-release-image.py')
module = importlib.util.module_from_spec(spec)
spec.loader.exec_module(module)


def fixture(target='server'):
    wanted = module.EXPECTED[target]
    config = {
        'User': 'dal:dal', 'Entrypoint': wanted['entrypoint'],
        'Healthcheck': {'Test': ['CMD', *wanted['health']]},
        'ExposedPorts': {'8080/tcp': {}} if target == 'server' else None,
        'Cmd': None, 'WorkingDir': '/tmp', 'Env': ['PATH=/usr/local/bin'],
    }
    meta = {'Config': config, 'Os': 'linux', 'Architecture': 'amd64'}
    packages = {'libc6:amd64': '2.41'} | {f'package-{i}:amd64': '1.0' for i in range(10)}
    files = {x: 'a' * 64 for x in wanted['binaries']}
    files['/usr/lib/x86_64-linux-gnu/libc.so.6'] = 'b' * 64
    sections = {'PRIVILEGES': []}
    return meta, packages, files, sections


class ReleaseImage(unittest.TestCase):
    def test_both_expected_runtimes(self):
        for target in ('server', 'worker'):
            with self.subTest(target=target):
                module.validate(target, *fixture(target))

    def test_binary_hash_and_package_version_changes_are_allowed(self):
        a = list(fixture('server'))
        a[1]['libc6:amd64'] = '2.42-security-update'
        a[2]['/usr/local/bin/dal-server'] = 'f' * 64
        module.validate('server', *a)

    def test_structural_security_breaks_fail_closed(self):
        meta, pkg, files, sections = fixture('server')
        for field, value in [('User', 'root'), ('Entrypoint', ['/bin/sh']),
                             ('Healthcheck', None), ('ExposedPorts', {'80/tcp': {}}),
                             ('Cmd', ['sh']), ('WorkingDir', '/root'),
                             ('Env', ['ACCESS_TOKEN=sensitive'])]:
            changed = {**meta, 'Config': {**meta['Config'], field: value}}
            with self.subTest(field=field), self.assertRaises(ValueError):
                module.validate('server', changed, pkg, files, sections)
        with self.assertRaises(ValueError):
            module.validate('server', meta, pkg, {**files, '/usr/local/bin/sh': 'a'*64}, sections)
        with self.assertRaises(ValueError):
            module.validate('server', meta, pkg, files, {'PRIVILEGES':['/usr/bin/su']})
        with self.assertRaises(ValueError):
            module.validate('server', meta, {}, files, sections)

    def test_worker_must_not_expose_ports(self):
        meta, pkg, files, sections = fixture('worker')
        meta['Config']['ExposedPorts'] = {'8080/tcp': {}}
        with self.assertRaises(ValueError):
            module.validate('worker', meta, pkg, files, sections)

    def test_receipt_has_archive_checksum_no_approval_secret(self):
        with tempfile.TemporaryDirectory() as tmp:
            f = Path(tmp) / 'image.oci'; f.write_bytes(b'synthetic')
            receipt = Path(tmp) / 'receipt.json'
            meta, pkgs, files, sec = fixture()
            with patch.object(module, 'capture', return_value=(meta,pkgs,files,sec)), \
                 patch.object(sys, 'argv', ['gate', '--target','server','--image','synthetic:server',
                                           '--archive',str(f),'--receipt',str(receipt)]):
                module.main()
            d = json.loads(receipt.read_text())
            self.assertEqual(d['target'], 'server')
            self.assertTrue(d['validated'])
            self.assertEqual(len(d['archive_sha256']), 64)
            self.assertNotIn('accepted_oci_digest', d)

    def test_workflow_requires_successful_main_push_and_security_scan(self):
        import yaml
        w = yaml.safe_load((ROOT/'.github/workflows/ghcr-manual.yaml').read_text())
        self.assertIn('workflow_run', w.get('on', w.get(True)))
        job = w['jobs']['image']
        condition = job['if']
        for term in ('conclusion', 'success', 'event', 'push', 'main', 'head_repository.full_name'):
            self.assertIn(term, condition)
        steps = job['steps']
        scan = next(i for i,s in enumerate(steps) if 'trivy-action@' in s.get('uses',''))
        validate = next(i for i,s in enumerate(steps) if 'check-release-image.py' in s.get('run',''))
        publish = next(i for i,s in enumerate(steps) if 'skopeo copy --all --preserve-digests' in s.get('run',''))
        self.assertLess(scan, validate)
        self.assertLess(validate, publish)
        self.assertEqual(steps[scan]['with']['exit-code'], '1')
        self.assertIn('CRITICAL', steps[scan]['with']['severity'])
        self.assertIn('sha256sum --check', steps[publish]['run'])
        self.assertIn('skopeo inspect --raw', steps[publish]['run'])
        self.assertIn('ghcr-release', job['environment'])
        self.assertNotIn('GHCR_RUNTIME_ACCEPTANCE', str(w))

if __name__ == '__main__':
    unittest.main()
