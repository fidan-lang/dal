"""Offline regression checks for the root-only DAL deployment receiver.

No Docker daemon, VPS access, network, or real credentials required.
"""
import base64
import importlib.util
import json
import os
from pathlib import Path
import stat
import tempfile
import unittest
from unittest.mock import patch
import subprocess
import sys

sys.dont_write_bytecode = True
ROOT = Path(__file__).resolve().parents[1]
SOURCE = ROOT / 'deploy' / 'dal-deploy.py'
spec = importlib.util.spec_from_file_location('dal_deploy_receiver', SOURCE)
deploy = importlib.util.module_from_spec(spec)
spec.loader.exec_module(deploy)


class ReceiverTests(unittest.TestCase):
    def test_temporary_auth_file_is_private_and_avoids_gpg_helpers(self):
        username = 'test-reader'
        token = 'synthetic-token-for-tests-only'
        with tempfile.TemporaryDirectory() as temp:
            private_dir = Path(temp) / 'docker-auth'
            private_dir.mkdir(mode=0o700)
            deploy.write_docker_auth(private_dir, username, token)
            f = private_dir / 'config.json'
            data = json.loads(f.read_text(encoding='utf-8'))
            self.assertEqual(set(data), {'auths'})
            self.assertEqual(set(data['auths']), {'ghcr.io'})
            self.assertNotIn('credsStore', data)
            self.assertNotIn('credHelpers', data)
            b64 = data['auths']['ghcr.io']['auth']
            self.assertEqual(base64.b64decode(b64).decode(), f'{username}:{token}')
            self.assertEqual(stat.S_IMODE(f.stat().st_mode), 0o600)
            self.assertEqual(stat.S_IMODE(private_dir.stat().st_mode), 0o700)
            # O_EXCL prevents accidentally overwriting an existing config.
            with self.assertRaises(FileExistsError):
                deploy.write_docker_auth(private_dir, username, token)

    def test_main_uses_temp_auth_and_digest_pulls_without_login(self):
        # Static ordering assertion supplements integration tests on the real VPS.
        source = SOURCE.read_text(encoding='utf-8')
        self.assertIn("write_docker_auth(Path(auth), payload['ghcr_user'], payload['ghcr_token'])", source)
        self.assertIn("run('docker', '--config', auth, 'pull', image, timeout=480)", source)
        self.assertNotIn("'login','ghcr.io'", source)
        self.assertIn("if run('stat','-f','-c','%T',str(RUNTIME)) != 'tmpfs'", source)
        self.assertIn("TemporaryDirectory(dir=RUNTIME, prefix='docker-auth-')", source)

    def test_failed_subprocess_excludes_stdout_stderr_and_auth_input(self):
        private_token = 'synthetic-secret-not-to-log'
        result = subprocess.CompletedProcess(args=['docker', 'pull'], returncode=1,
                                             stdout='stdout ' + private_token,
                                             stderr='stderr ' + private_token)
        with patch.object(deploy.subprocess, 'run', return_value=result):
            with self.assertRaises(RuntimeError) as error:
                deploy.run('docker', 'pull', 'image', input_text=private_token)
        self.assertNotIn(private_token, str(error.exception))
        self.assertNotIn('stdout', str(error.exception))
        self.assertNotIn('stderr', str(error.exception))

    def test_image_updates_require_exact_keys_and_preserve_other_config(self):
        original = b'UNCHANGED=value\nDAL_SERVER_IMAGE=old-server\nDAL_WORKER_IMAGE=old-worker\n'
        actual = deploy.update_image_lines(original, 'new-server', 'new-worker')
        self.assertEqual(actual, b'UNCHANGED=value\nDAL_SERVER_IMAGE=new-server\nDAL_WORKER_IMAGE=new-worker\n')
        for corrupt in (b'DAL_SERVER_IMAGE=one\n',
                        original + b'DAL_SERVER_IMAGE=duplicate\n'):
            with self.assertRaises(RuntimeError):
                deploy.update_image_lines(corrupt, 'server', 'worker')


if __name__ == '__main__':
    unittest.main()
