"""Synthetic-only checks for protected acceptance and prepublication equivalence."""
import copy
import importlib.util
import io
import json
import os
from pathlib import Path
import sys
import tempfile
import unittest
from contextlib import redirect_stdout
from unittest.mock import patch

sys.dont_write_bytecode = True
ROOT = Path(__file__).resolve().parents[1]
spec = importlib.util.spec_from_file_location('release_image', ROOT / 'scripts/check-release-image.py')
module = importlib.util.module_from_spec(spec)
spec.loader.exec_module(module)
SECRET = 'GHCR_RUNTIME_ACCEPTANCE'


def runtime_fixture():
    return {'platform': 'linux/amd64', 'config': {'User': 'dal:dal', 'Env': ['PATH=/synthetic/bin']},
            'packages_sha256': 'a' * 64, 'native_files_sha256': 'b' * 64,
            'library_links_sha256': 'c' * 64, 'binary_modes_sha256': 'd' * 64,
            'binaries': {'/usr/local/bin/example': 'e' * 64},
            'glibc_packages': {'libc6:amd64': 'synthetic-version'},
            'installed_packages': 2, 'native_files': 3, 'privilege_bearing_files': 0}


def acceptance_fixture():
    # Deliberately invented IDs/hashes: no real acceptance or production inventory.
    return {'scope': 'private-publication-only', 'owner': 'AppSolves',
            'accepted_glibc_cves': ['CVE-0000-0001'], 'deployment_approved': False,
            'images': {name: {'accepted_oci_digest': 'sha256:' + letter * 64, 'runtime': runtime_fixture()}
                       for name, letter in [('server', 'a'), ('worker', 'b')]}}


class ReleaseImage(unittest.TestCase):
    def setUp(self):
        self.runtime = runtime_fixture()

    def test_fingerprints_ignore_map_order_but_not_changed_content(self):
        self.assertEqual(module.fingerprint({'a': 1, 'b': 2}), module.fingerprint({'b': 2, 'a': 1}))
        self.assertNotEqual(module.fingerprint({'a': 1}), module.fingerprint({'a': 2}))

    def test_identical_runtime_is_accepted(self):
        module.verify(copy.deepcopy(self.runtime), self.runtime)

    def test_every_changed_runtime_boundary_blocks_publication(self):
        for field in self.runtime:
            with self.subTest(field=field):
                changed = copy.deepcopy(self.runtime)
                changed[field] = 'changed'
                with self.assertRaisesRegex(ValueError, field): module.verify(changed, self.runtime)

    def test_added_or_missing_evidence_blocks(self):
        with self.assertRaises(ValueError): module.verify({**self.runtime, 'unknown': 'value'}, self.runtime)
        changed = copy.deepcopy(self.runtime)
        del changed['binaries']
        with self.assertRaises(ValueError): module.verify(changed, self.runtime)

    def test_rejection_does_not_dump_sensitive_candidate_environment(self):
        changed = copy.deepcopy(self.runtime)
        changed['config']['Env'] = ['TOKEN=synthetic-secret']
        with self.assertRaises(ValueError) as error: module.verify(changed, self.runtime)
        self.assertNotIn('synthetic-secret', str(error.exception))

    def test_secret_is_required_without_repository_fallback(self):
        for value in (None, '', '{synthetic-secret', '[]', '{}'):
            with self.subTest(value=value), patch.dict(os.environ, {}, clear=True):
                if value is not None: os.environ[SECRET] = value
                with self.assertRaises(ValueError) as error: module.load_acceptance()
                self.assertNotIn('synthetic-secret', str(error.exception))
        self.assertFalse((ROOT / 'config/container-release-acceptance.json').exists())

    def test_valid_secret_is_consumed_before_subprocesses(self):
        fixture = acceptance_fixture()
        with patch.dict(os.environ, {SECRET: json.dumps(fixture)}, clear=True):
            self.assertEqual(module.load_acceptance(), fixture)
            self.assertNotIn(SECRET, os.environ)

    def test_invalid_scope_owner_or_incomplete_runtime_fails_closed(self):
        fixtures = []
        for name, value in [('scope', 'deployment'), ('owner', 'unapproved'), ('deployment_approved', True),
                            ('accepted_glibc_cves', []), ('images', {})]:
            fixture = acceptance_fixture(); fixture[name] = value; fixtures.append(fixture)
        fixture = acceptance_fixture(); del fixture['images']['worker']['runtime']['packages_sha256']; fixtures.append(fixture)
        fixture = acceptance_fixture(); fixture['images']['server']['runtime']['config']['User'] = 'root'; fixtures.append(fixture)
        for fixture in fixtures:
            with self.subTest(fixture=fixture), patch.dict(os.environ, {SECRET: json.dumps(fixture)}, clear=True):
                with self.assertRaises(ValueError): module.load_acceptance()

    def test_validation_only_checks_secret_without_inspecting_images(self):
        with patch.dict(os.environ, {SECRET: json.dumps(acceptance_fixture())}, clear=True), \
                patch.object(sys, 'argv', ['gate', '--validate-acceptance']), \
                patch.object(module, 'capture') as capture, redirect_stdout(io.StringIO()):
            module.main()
            capture.assert_not_called()

    def test_receipt_and_stdout_exclude_private_acceptance(self):
        fixture = acceptance_fixture()
        fixture['private_note'] = 'synthetic-private-marker'
        def capture(image):
            self.assertNotIn(SECRET, os.environ)
            return runtime_fixture()
        with tempfile.TemporaryDirectory() as directory:
            archive = Path(directory) / 'synthetic.oci'; archive.write_bytes(b'synthetic archive')
            receipt = Path(directory) / 'receipt.json'; output = io.StringIO()
            with patch.dict(os.environ, {SECRET: json.dumps(fixture)}, clear=True), \
                    patch.object(sys, 'argv', ['gate', '--target', 'server', '--image', 'synthetic:server',
                                              '--archive', str(archive), '--receipt', str(receipt)]), \
                    patch.object(module, 'capture', side_effect=capture), redirect_stdout(output):
                module.main()
            body = json.loads(receipt.read_text())
            self.assertEqual(set(body), {'target', 'equivalent', 'archive_sha256', 'scope', 'deployment_approved'})
            combined = receipt.read_text() + output.getvalue()
            for private in ('synthetic-private-marker', 'CVE-0000-0001', 'synthetic-version',
                            fixture['images']['server']['accepted_oci_digest'], self.runtime['native_files_sha256']):
                self.assertNotIn(private, combined)
            self.assertTrue(body['equivalent'])

    def test_workflow_keeps_secret_scoped_and_receipt_out_of_public_artifacts(self):
        import yaml
        workflow = yaml.safe_load((ROOT / '.github/workflows/ghcr-manual.yaml').read_text())
        job = workflow['jobs']['image']; steps = job['steps']
        validate = next(i for i, s in enumerate(steps) if '--validate-acceptance' in s.get('run', ''))
        build = next(i for i, s in enumerate(steps) if s.get('id') == 'image')
        verify = next(i for i, s in enumerate(steps) if '--receipt "$RUNTIME_RECEIPT"' in s.get('run', ''))
        push = next(i for i, s in enumerate(steps) if 'skopeo copy --all --preserve-digests' in s.get('run', ''))
        self.assertLess(validate, build); self.assertLess(build, verify); self.assertLess(verify, push)
        self.assertIs(steps[build]['with']['push'], False)
        self.assertIn('type=oci', steps[build]['with']['outputs'])
        self.assertEqual(steps[build]['env']['DOCKER_BUILD_RECORD_UPLOAD'], 'false')
        self.assertEqual(steps[build]['env']['DOCKER_BUILD_SUMMARY'], 'false')
        self.assertIn('--quiet', steps[push]['run'])
        self.assertTrue(steps[build]['with']['sbom']); self.assertEqual(steps[build]['with']['provenance'], 'mode=max')
        self.assertEqual(steps[push]['if'], 'inputs.publish'); self.assertIn('sha256sum --check', steps[push]['run'])
        self.assertNotIn(SECRET, job['env'])
        for index in (validate, verify):
            self.assertEqual(steps[index]['env'][SECRET], '${{ secrets.GHCR_RUNTIME_ACCEPTANCE }}')
        uploader = next(s for s in steps if s.get('name') == 'Preserve visibility and rollback identity receipts')
        from fnmatch import fnmatch
        self.assertFalse(fnmatch(job['env']['RUNTIME_RECEIPT'], uploader['with']['path']))
        cleanup = next(s for s in steps if s.get('name') == 'Remove private build files')
        self.assertEqual(cleanup['if'], 'always()')
        self.assertIn('"$RUNTIME_RECEIPT"', cleanup['run'])
        self.assertIn('"$IMAGE_ARCHIVE"', cleanup['run'])


if __name__ == '__main__': unittest.main()
