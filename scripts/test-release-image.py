"""Fail-closed regression checks for the prepublication runtime equivalence gate."""
import copy
import importlib.util
import json
from pathlib import Path
import unittest
import sys
sys.dont_write_bytecode = True

ROOT = Path(__file__).resolve().parents[1]
spec = importlib.util.spec_from_file_location('release_image', ROOT / 'scripts/check-release-image.py')
module = importlib.util.module_from_spec(spec)
spec.loader.exec_module(module)


class ReleaseImage(unittest.TestCase):
    def setUp(self):
        self.runtime = {'platform': 'linux/amd64', 'config': {'User': 'dal:dal', 'Env': ['PATH=/usr/bin']},
                        'packages': {'libc6:amd64': 'accepted'}, 'files': {'/usr/local/bin/dal-server': 'hash'},
                        'links': [], 'binary_modes': [], 'privilege_bearing_files': 0}

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
                with self.assertRaisesRegex(ValueError, field):
                    module.verify(changed, self.runtime)

    def test_added_or_missing_evidence_blocks(self):
        changed = {**self.runtime, 'unknown': 'value'}
        with self.assertRaises(ValueError): module.verify(changed, self.runtime)
        changed = copy.deepcopy(self.runtime)
        del changed['files']
        with self.assertRaises(ValueError): module.verify(changed, self.runtime)

    def test_rejection_does_not_dump_sensitive_candidate_environment(self):
        changed = copy.deepcopy(self.runtime)
        changed['config']['Env'] = ['TOKEN=synthetic-secret']
        with self.assertRaises(ValueError) as error: module.verify(changed, self.runtime)
        self.assertNotIn('synthetic-secret', str(error.exception))

    def test_workflow_builds_and_verifies_before_any_push(self):
        import yaml
        workflow = yaml.safe_load((ROOT / '.github/workflows/ghcr-manual.yaml').read_text())
        steps = workflow['jobs']['image']['steps']
        build = next(i for i, s in enumerate(steps) if s.get('id') == 'image')
        verify = next(i for i, s in enumerate(steps) if 'check-release-image.py' in s.get('run', ''))
        push = next(i for i, s in enumerate(steps) if 'skopeo copy --all --preserve-digests' in s.get('run', ''))
        self.assertLess(build, verify)
        self.assertLess(verify, push)
        self.assertIs(steps[build]['with']['push'], False)
        self.assertIn('type=oci', steps[build]['with']['outputs'])
        self.assertIn('sha256sum --check', steps[push]['run'])
        self.assertEqual(steps[push]['if'], 'inputs.publish')
        self.assertTrue(steps[build]['with']['sbom'])
        self.assertEqual(steps[build]['with']['provenance'], 'mode=max')

    def test_acceptance_is_scoped_to_exact_reviewed_images_and_cves(self):
        acceptance = json.loads((ROOT / 'config/container-release-acceptance.json').read_text())
        self.assertEqual(acceptance['scope'], 'private-publication-only')
        self.assertEqual(acceptance['owner'], 'AppSolves')
        self.assertEqual(len(set(acceptance['accepted_glibc_cves'])), 17)
        self.assertEqual(set(acceptance['images']), {'server', 'worker'})
        for image in acceptance['images'].values():
            self.assertRegex(image['accepted_oci_digest'], r'^sha256:[a-f0-9]{64}$')
            self.assertEqual(image['runtime']['privilege_bearing_files'], 0)
            self.assertIn(image['runtime']['config']['User'], ('dal:dal', '10001:10001'))


if __name__ == '__main__': unittest.main()
