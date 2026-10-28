"""Offline GHCR publication-policy regressions; never publishes or changes settings."""
import importlib.util
from pathlib import Path
import unittest
import sys
sys.dont_write_bytecode = True

spec = importlib.util.spec_from_file_location("private_ghcr", Path(__file__).with_name("check-private-ghcr.py"))
policy = importlib.util.module_from_spec(spec)
spec.loader.exec_module(policy)

class PrivateImages(unittest.TestCase):
    def test_workflow_cannot_publish_from_pr_or_ordinary_push_or_before_checks(self):
        import yaml
        workflow = yaml.safe_load((Path(__file__).parents[1] / ".github/workflows/ghcr-manual.yaml").read_text())
        self.assertEqual(set(workflow.get("on", workflow.get(True))), {"workflow_dispatch"})
        job = workflow["jobs"]["image"]
        self.assertEqual(job["environment"], "ghcr-release")
        self.assertEqual(job["permissions"]["packages"], "read")
        steps = job["steps"]
        guard = next(i for i, step in enumerate(steps) if "check-private-ghcr.py --receipt" in step.get("run", ""))
        login = next(i for i, step in enumerate(steps) if "docker/login-action@" in step.get("uses", ""))
        self.assertLess(guard, login)
        self.assertEqual(steps[guard]["if"], "inputs.publish")
        for step in steps:
            if "docker/login-action@" in step.get("uses", ""):
                self.assertNotIn("GITHUB_TOKEN", step["with"]["password"])
        build = next(step for step in steps if "docker/build-push-action@" in step.get("uses", ""))
        self.assertEqual(build["with"]["platforms"], "linux/amd64", "Only the natively accepted image architecture may be published")
        self.assertNotIn("org.opencontainers.image.source", build["with"]["labels"])
        self.assertEqual(build["with"]["push"], "${{ inputs.publish }}")
        self.assertNotIn("cache-to", build["with"], "Private image layers must not enter a public repository cache")
        self.assertNotIn("cache-from", build["with"])

    def test_production_runtime_credentials_are_separate(self):
        import yaml
        compose=yaml.safe_load((Path(__file__).parents[1] / "deploy/compose.production.yaml").read_text())
        server=compose["services"]["server"]["env_file"]
        worker=compose["services"]["worker"]["env_file"]
        self.assertIn("DAL_API_ENV_FILE",server)
        self.assertIn("DAL_WORKER_ENV_FILE",worker)
        self.assertNotEqual(server,worker)

    def test_only_explicit_private_packages_are_accepted(self):
        for value in ("public", "internal", None):
            with self.assertRaises(ValueError):
                policy.validate_package({"id": 1, "name": "dal-server", "visibility": value}, "dal-server")
        self.assertEqual(policy.validate_package({"id": 1, "name": "dal-server", "visibility": "private"}, "dal-server"), 1)
        with self.assertRaises(ValueError):
            policy.validate_package({"id": 1, "name": "other", "visibility": "private"}, "dal-server")

    def test_environment_must_have_real_review_and_main_only_policy(self):
        environment = {"can_admins_bypass": False, "deployment_branch_policy": {"custom_branch_policies": True}, "protection_rules": [{"type": "required_reviewers", "prevent_self_review": True, "reviewers": [{"type": "User", "reviewer": {"id": 1}}]}]}
        branches = {"branch_policies": [{"name": "main", "type": "branch"}]}
        policy.validate_environment(environment, branches)
        for change in ({"can_admins_bypass": True}, {"protection_rules": []}, {"deployment_branch_policy": None}):
            with self.assertRaises(ValueError): policy.validate_environment({**environment, **change}, branches)
        with self.assertRaises(ValueError): policy.validate_environment(environment, {"branch_policies": [{"name": "*", "type": "branch"}]})

    def test_manual_acl_review_is_bound_to_the_actual_package_ids(self):
        policy.validate_access_review("PRIVATE_ACCESS_REVIEWED:10:20", [10, 20])
        for value in ("", "true", "PRIVATE_ACCESS_REVIEWED:11:20"):
            with self.assertRaises(ValueError): policy.validate_access_review(value, [10, 20])

    def test_anonymous_token_is_followed_through_to_the_real_manifest_status(self):
        from unittest.mock import patch
        digest = "sha256:" + "a" * 64
        with patch.object(policy, "request", side_effect=[(200, b'{"token":"synthetic-anonymous-capability"}'), (200, b'{}')]) as requests:
            with self.assertRaises(ValueError): policy.anonymous_denied("dal-server", digest)
            self.assertEqual(requests.call_args_list[1].args[1], "synthetic-anonymous-capability")
        with patch.object(policy, "request", side_effect=[(200, b'{"token":"synthetic-anonymous-capability"}'), (401, b'')]):
            policy.anonymous_denied("dal-server", digest)

    def test_anonymous_pull_requires_denial_and_fails_on_network_errors(self):
        for status in (401, 403, 404): policy.validate_anonymous_status(status)
        for status in (200, 301, 429, 500):
            with self.assertRaises(ValueError): policy.validate_anonymous_status(status)

if __name__ == "__main__": unittest.main()
