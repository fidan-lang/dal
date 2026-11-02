"""Offline regression tests for Roles Anywhere packaging; no AWS APIs or credentials needed."""
import hashlib
import json
import os
from pathlib import Path
import shutil
import subprocess
import tempfile
import unittest
import uuid

import yaml

ROOT = Path(__file__).resolve().parents[1]
CONFIG = json.loads((ROOT / 'deploy/roles-anywhere.json').read_text())


class VpsAwsPolicy(unittest.TestCase):
    def test_separate_identities_and_pinned_official_helpers(self):
        api, worker = (CONFIG['workloads'][name] for name in ('api', 'worker'))
        self.assertNotEqual(api['cn'], worker['cn'])
        self.assertNotEqual(api['role'], worker['role'])
        self.assertNotEqual(api['profile'], worker['profile'])
        self.assertEqual(CONFIG['session_seconds'], 900)
        self.assertGreater(CONFIG['dlq_retention_seconds'], 345600)
        self.assertEqual(CONFIG['max_receive_count'], 5)
        for platform in ['linux_amd64', 'windows_amd64']:
            helper = CONFIG['helper'][platform]
            self.assertTrue(helper['url'].startswith('https://rolesanywhere.amazonaws.com/releases/' + CONFIG['helper']['version'] + '/'))
            self.assertRegex(helper['sha256'], r'^[0-9a-f]{64}$')

    def test_real_compose_merge_preserves_runtime_boundaries(self):
        with tempfile.TemporaryDirectory(prefix='vps-compose-', dir=ROOT / 'target') as name:
            root = Path(name)
            for part in ['api', 'worker']:
                (root / part).mkdir()
                (root / (part + '.env')).write_text('DATABASE_URL=not-a-real-database\nAWS_ACCESS_KEY_ID=synthetic-must-be-removed\nAWS_SECRET_ACCESS_KEY=synthetic-must-be-removed\nAWS_SESSION_TOKEN=synthetic-must-be-removed\n')
            (root / 'helper').write_text('fixture')
            env = dict(os.environ, DAL_SERVER_IMAGE='server@sha256:' + 'a' * 64,
                       DAL_WORKER_IMAGE='worker@sha256:' + 'b' * 64,
                       DAL_API_ENV_FILE=str(root / 'api.env'), DAL_WORKER_ENV_FILE=str(root / 'worker.env'),
                       DAL_AWS_HELPER=str(root / 'helper'), DAL_API_AWS_DIRECTORY=str(root / 'api'),
                       DAL_WORKER_AWS_DIRECTORY=str(root / 'worker'))
            result = subprocess.run(['docker', 'compose', '-f', 'deploy/compose.production.yaml', '-f', 'deploy/compose.roles-anywhere.yaml', 'config', '--format', 'json'], cwd=ROOT, env=env, capture_output=True, text=True, timeout=30)
            self.assertEqual(result.returncode, 0, result.stderr)
            merged = json.loads(result.stdout)['services']
            for name, profile in [('server', 'dal-vps-api'), ('worker', 'dal-vps-worker')]:
                service = merged[name]
                self.assertTrue(service['read_only'])
                self.assertEqual(service['cap_drop'], ['ALL'])
                self.assertIn('no-new-privileges:true', service['security_opt'])
                self.assertEqual(int(service['mem_limit']), 512 * 1024 * 1024)
                self.assertEqual(service['environment']['AWS_PROFILE'], profile)
                self.assertEqual(service['environment']['AWS_EC2_METADATA_DISABLED'], 'true')
                self.assertEqual(service['environment']['AWS_SHARED_CREDENTIALS_FILE'], '/dev/null')
                for key in ['AWS_ACCESS_KEY_ID', 'AWS_SECRET_ACCESS_KEY', 'AWS_SESSION_TOKEN']:
                    self.assertEqual(service['environment'][key], '')
                mounts = service['volumes']
                self.assertEqual(len(mounts), 2)
                self.assertTrue(all(mount['read_only'] for mount in mounts))
                self.assertTrue(all(mount['bind']['create_host_path'] is False for mount in mounts))
                credential_mount = next(m for m in mounts if m['target'] == '/run/dal-aws')
                self.assertEqual(Path(credential_mount['source']).resolve(), (root / ('api' if name == 'server' else 'worker')).resolve())
            self.assertEqual(merged['server']['ports'][0]['host_ip'], '127.0.0.1')

    @unittest.skipUnless(os.name == 'nt', 'Windows-only offline CA issuance; Debian runtime tested separately')
    def test_certificate_preparation_is_private_and_idempotent(self):
        executable = shutil.which('pwsh')
        self.assertIsNotNone(executable)
        with tempfile.TemporaryDirectory(prefix='vps-issuer-', dir=ROOT / 'target') as name:
            root = Path(name)
            resources = {'account': CONFIG['account'], 'region': CONFIG['region'],
                         'trust_anchor_arn': f"arn:aws:rolesanywhere:{CONFIG['region']}:{CONFIG['account']}:trust-anchor/00000000-0000-0000-0000-000000000001",
                         'profiles': {part: f"arn:aws:rolesanywhere:{CONFIG['region']}:{CONFIG['account']}:profile/00000000-0000-0000-0000-00000000000{n}" for n, part in enumerate(['api', 'worker'], 2)}}
            (root / 'resources.json').write_text(json.dumps(resources))
            command = [executable, '-NoProfile', '-File', str(ROOT / 'scripts/prepare-vps-aws.ps1'), '-CaDirectory', str(root / 'ca'), '-OutputDirectory', str(root / 'bundle'), '-ResourcesFile', str(root / 'resources.json')]
            def issue(allowed=0):
                result = subprocess.run(command, cwd=ROOT, capture_output=True, text=True, timeout=30)
                self.assertEqual(result.returncode, allowed, result.stderr if allowed == 0 else 'Expected input rejection')
                self.assertNotIn('PRIVATE KEY', result.stdout)
                return result
            issue()
            def hashes():
                return {str(p.relative_to(root)): hashlib.sha256(p.read_bytes()).hexdigest() for p in root.rglob('*') if p.is_file() and p.name != 'resources.json'}
            before = hashes()
            issue()
            self.assertEqual(before, hashes(), 'Repeated configuration must not replace CA or workload identities')
            self.assertFalse(list((root / 'bundle').rglob('*.pfx')))
            self.assertTrue((root / 'ca/issuer-password.dpapi').stat().st_size > 32)
            resources['profiles']['worker'] = resources['profiles']['api']
            (root / 'resources.json').write_text(json.dumps(resources))
            result = subprocess.run(command, cwd=ROOT, capture_output=True, text=True, timeout=30)
            self.assertNotEqual(result.returncode, 0)
            self.assertIn('distinct', result.stderr)
            self.assertEqual(before, hashes())
            for field, value in [('trust_anchor_arn', resources['trust_anchor_arn'].replace(CONFIG['region'], 'us-east-1'))]:
                invalid = dict(resources)
                invalid[field] = value
                (root / 'resources.json').write_text(json.dumps(invalid))
                result = subprocess.run(command, cwd=ROOT, capture_output=True, text=True, timeout=30)
                self.assertNotEqual(result.returncode, 0)
                self.assertIn('resource identity', result.stderr)
                self.assertEqual(before, hashes())
            nested = list(command)
            nested[nested.index('-OutputDirectory') + 1] = str(root / 'ca/nested')
            result = subprocess.run(nested, cwd=ROOT, capture_output=True, text=True, timeout=30)
            self.assertNotEqual(result.returncode, 0)
            self.assertFalse((root / 'ca/nested').exists())
            unignored = ROOT / ('vps-unignored-' + uuid.uuid4().hex)
            unsafe = list(command)
            unsafe[unsafe.index('-OutputDirectory') + 1] = str(unignored)
            result = subprocess.run(unsafe, cwd=ROOT, capture_output=True, text=True, timeout=30)
            self.assertNotEqual(result.returncode, 0)
            self.assertIn('Git-ignored', result.stderr)
            self.assertFalse(unignored.exists())


if __name__ == '__main__':
    unittest.main()
