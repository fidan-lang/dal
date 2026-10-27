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

    def test_cache_never_selects_authenticated_or_non_read_requests(self):
        for rule in edge.prepare("api.dal.fidan.dev")["cache_rules"]:
            self.assertIn('http.request.method in {"GET" "HEAD"}', rule["expression"])
            for header in ["authorization", "cookie"]:
                self.assertIn('not any(http.request.headers["' + header + '"][*] ne "")', rule["expression"])
            self.assertEqual(rule["action_parameters"]["edge_ttl"]["status_code_ttl"],
                             [{"status_code_range": {"from": 300, "to": 599}, "value": -1}])

    def test_preparation_is_idempotent_and_preserves_unrelated_rules(self):
        unrelated = {"ref": "keep_security", "action": "set_cache_settings", "expression": "false"}
        existing = {"phase": "http_request_cache_settings", "rules": [unrelated]}
        first = edge.prepare("api.dal.fidan.dev", existing)
        self.assertEqual(len(first["cache_rules_to_add"]), 2)
        self.assertEqual(existing["rules"], [unrelated])
        existing["rules"] += first["cache_rules_to_add"]
        self.assertEqual(edge.prepare("api.dal.fidan.dev", existing)["cache_rules_to_add"], [])
        existing["rules"][1] = {**existing["rules"][1], "expression": "true"}
        with self.assertRaises(ValueError): edge.prepare("api.dal.fidan.dev", existing)
        with self.assertRaises(ValueError): edge.prepare("api.dal.fidan.dev", {"phase": "http_ratelimit", "rules": []})

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
        self.assertNotIn("authorization", value["additional_rate_rule"]["expression"], "credentials must not bypass the quota")
        self.assertNotIn("cookie", value["additional_rate_rule"]["expression"])


if __name__ == "__main__": unittest.main()
