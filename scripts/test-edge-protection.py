import sys
sys.dont_write_bytecode = True
import importlib.util
import json
from pathlib import Path
import unittest

ROOT = Path(__file__).resolve().parents[1]
spec = importlib.util.spec_from_file_location("edge", ROOT / "scripts/prepare-edge-protection.py")
edge = importlib.util.module_from_spec(spec)
spec.loader.exec_module(edge)


class EdgePolicyTests(unittest.TestCase):
    def test_hostnames_cannot_inject_rules_or_select_other_paths(self):
        for value in ('" or true', 'https://api.example.test', 'api.example.test/path', 'api.example.test:443', '*.example.test', 'UPPER.example.test'):
            with self.assertRaises(ValueError): edge.prepare(value)

    def test_rules_preserve_signed_urls_and_do_not_cache_mutable_registry_state(self):
        value = edge.prepare("api.dal.fidan.dev")
        self.assertEqual(value["status"], "REVIEW_ONLY_NOT_APPLIED")
        self.assertTrue(value["preserve_existing_rules"])
        release, packages = value["cache_rules"]
        self.assertNotIn("manifest.json", release["expression"])
        self.assertIn('".tar.gz"', release["expression"])
        self.assertIn('"/download"', packages["expression"])
        self.assertNotIn('"/index/"', packages["expression"])
        self.assertEqual(packages["action_parameters"]["edge_ttl"]["mode"], "bypass_by_default")
        for rule in (release, packages):
            self.assertTrue(rule["action_parameters"]["cache_key"]["custom_key"]["query_string"]["exclude"]["all"])
        self.assertEqual(value["additional_rate_rule"]["action"], "block")
        self.assertEqual(value["additional_rate_rule"]["ratelimit"]["requests_per_period"], json.loads((ROOT / "config/public-read-policy.json").read_text())["download_requests_per_minute"])
        self.assertNotIn("managed_challenge", json.dumps(value))


if __name__ == "__main__": unittest.main()
