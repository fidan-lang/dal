"""Offline GHCR publication-policy regressions; never publishes or changes settings."""
import importlib.util
import json
import os
from pathlib import Path
import sys
import tempfile
import unittest
from unittest.mock import patch
sys.dont_write_bytecode = True

spec = importlib.util.spec_from_file_location("private_ghcr", Path(__file__).with_name("check-private-ghcr.py"))
policy = importlib.util.module_from_spec(spec)
spec.loader.exec_module(policy)


def environment():
    return {"name": "ghcr-release", "can_admins_bypass": False,
            "deployment_branch_policy": {"custom_branch_policies": True, "protected_branches": False},
            "protection_rules": [{"type": "required_reviewers", "prevent_self_review": False,
                                  "reviewers": [{"type": "User", "reviewer": {"id": 1, "login": "AppSolves"}}]}]}


def branches():
    return {"branch_policies": [{"name": "main", "type": "branch"}]}


class PrivateImages(unittest.TestCase):
    def test_workflow_keeps_privacy_gates_and_uses_nested_image_names(self):
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
        self.assertEqual(build["with"]["platforms"], "linux/amd64")
        self.assertTrue(build["with"]["tags"].startswith("ghcr.io/fidan-lang/dal/${{ matrix.target }}:"))
        self.assertNotIn("org.opencontainers.image.source", build["with"]["labels"])
        self.assertIs(build["with"]["push"], False)
        self.assertNotIn("cache-to", build["with"])
        self.assertNotIn("cache-from", build["with"])
        pull = next(step for step in steps if step.get("name") == "Verify authorized immutable pull")
        self.assertIn('ghcr.io/fidan-lang/dal/$IMAGE_TARGET@$IMAGE_DIGEST', pull["run"])

    def test_bootstrap_contains_metadata_only(self):
        source = (Path(__file__).parents[1] / "deploy/ghcr-bootstrap.Dockerfile").read_text()
        lines = [line.strip() for line in source.splitlines() if line.strip()]
        self.assertEqual(lines[0], "FROM scratch")
        self.assertTrue(all(line.startswith("LABEL ") for line in lines[1:]))
        self.assertNotIn("org.opencontainers.image.source", source)

    def test_production_runtime_credentials_are_separate(self):
        import yaml
        compose = yaml.safe_load((Path(__file__).parents[1] / "deploy/compose.production.yaml").read_text())
        self.assertIn("DAL_API_ENV_FILE", compose["services"]["server"]["env_file"])
        self.assertIn("DAL_WORKER_ENV_FILE", compose["services"]["worker"]["env_file"])

    def test_only_explicit_private_nested_packages_are_accepted(self):
        self.assertEqual(policy.PACKAGES, ("dal/server", "dal/worker"))
        for value in ("public", "internal", None):
            with self.assertRaises(ValueError):
                policy.validate_package({"id": 1, "name": "dal/server", "visibility": value}, "dal/server")
        self.assertEqual(policy.validate_package({"id": 1, "name": "dal/server", "visibility": "private"}, "dal/server"), 1)
        with self.assertRaises(ValueError):
            policy.validate_package({"id": 1, "name": "dal-server", "visibility": "private"}, "dal/server")

    def test_owner_self_approval_is_explicit_and_bypass_is_forbidden(self):
        policy.validate_environment(environment(), branches())
        for changes in ({"name": "other"}, {"can_admins_bypass": True}, {"protection_rules": []},
                        {"deployment_branch_policy": None}):
            with self.subTest(changes=changes), self.assertRaises(ValueError):
                policy.validate_environment({**environment(), **changes}, branches())
        for value in (True, None):
            candidate = environment(); candidate["protection_rules"][0]["prevent_self_review"] = value
            with self.subTest(self_review=value), self.assertRaises(ValueError):
                policy.validate_environment(candidate, branches())
        candidate = environment(); candidate["deployment_branch_policy"]["protected_branches"] = True
        with self.assertRaises(ValueError): policy.validate_environment(candidate, branches())
        for reviewer in ({"type": "Team", "reviewer": {"login": "AppSolves"}},
                         {"type": "User", "reviewer": {"login": "someone-else"}}):
            candidate = environment(); candidate["protection_rules"][0]["reviewers"] = [reviewer]
            with self.assertRaises(ValueError): policy.validate_environment(candidate, branches())
        candidate = environment(); candidate["protection_rules"][0]["reviewers"] *= 2
        with self.assertRaises(ValueError): policy.validate_environment(candidate, branches())
        for rule in ({"name": "*", "type": "branch"}, {"name": "main", "type": "tag"}):
            with self.assertRaises(ValueError):
                policy.validate_environment(environment(), {"branch_policies": [rule]})

    def test_only_the_active_sole_owner_is_authorized(self):
        membership = {"role": "admin", "state": "active"}
        owners = [{"login": "AppSolves"}]
        policy.validate_owner(membership, owners)
        for value in ({"role": "member", "state": "active"}, {"role": "admin", "state": "pending"}):
            with self.assertRaises(ValueError): policy.validate_owner(value, owners)
        for value in ([], [{"login": "other"}], owners + [{"login": "other"}]):
            with self.assertRaises(ValueError): policy.validate_owner(membership, value)

    def test_main_encodes_rest_names_and_checks_the_requested_target_digest(self):
        digest = "sha256:" + "b" * 64
        paths = {
            "repos/fidan-lang/dal/environments/ghcr-release": environment(),
            "repos/fidan-lang/dal/environments/ghcr-release/deployment-branch-policies": branches(),
            "orgs/fidan-lang/memberships/AppSolves": {"role": "admin", "state": "active"},
            "orgs/fidan-lang/members?role=admin&per_page=2": [{"login": "AppSolves"}],
        }
        for name, identity in [("dal/server", 10), ("dal/worker", 20)]:
            base = "orgs/fidan-lang/packages/container/" + name.replace("/", "%2F")
            paths[base] = {"id": identity, "name": name, "visibility": "private"}
            paths[base + "/versions?per_page=1"] = [{"name": "sha256:" + "a" * 64}]
        requested = []
        def request(url, token=None):
            requested.append(url)
            path = url.removeprefix("https://api.github.com/")
            if path not in paths: raise AssertionError("Unexpected or unencoded GitHub URL: " + url)
            return 200, json.dumps(paths[path]).encode()
        env = {"GITHUB_REPOSITORY": "fidan-lang/dal", "GITHUB_REF": "refs/heads/main", "GH_TOKEN": "synthetic-inspection",
               "GHCR_PUBLISH_TOKEN": "synthetic-publisher", "GHCR_PULL_TOKEN": "synthetic-reader",
               "GHCR_PRIVATE_ACCESS_REVIEW": "PRIVATE_ACCESS_REVIEWED:10:20"}
        with tempfile.TemporaryDirectory() as folder:
            receipt = Path(folder) / "receipt.json"
            with patch.dict(os.environ, env, clear=True), patch.object(sys, "argv", ["gate", "--target", "worker", "--digest", digest, "--receipt", str(receipt)]), patch.object(policy, "request", side_effect=request), patch.object(policy, "anonymous_denied") as anonymous:
                policy.main()
            self.assertEqual([c.args for c in anonymous.call_args_list], [("dal/server", "sha256:" + "a" * 64), ("dal/worker", digest)])
            self.assertEqual([p["name"] for p in json.loads(receipt.read_text())["packages"]], ["dal/server", "dal/worker"])
            receipt.unlink()
            for name in ("dal/server", "dal/worker"):
                missing = "https://api.github.com/orgs/fidan-lang/packages/container/" + name.replace("/", "%2F")
                def absent(url, token=None):
                    return (404, b"") if url == missing else request(url, token)
                with patch.dict(os.environ, env, clear=True), patch.object(sys, "argv", ["gate", "--receipt", str(receipt)]), patch.object(policy, "request", side_effect=absent), patch.object(policy, "anonymous_denied"):
                    with self.assertRaises(ValueError): policy.main()
                self.assertFalse(receipt.exists(), "Missing packages must not receive a successful receipt")
        self.assertTrue(any("dal%2Fserver/versions" in url for url in requested))
        self.assertTrue(any("dal%2Fworker/versions" in url for url in requested))

    def test_invalid_digest_fails_before_network_access(self):
        for value in ("latest", "sha256:abc", "sha256:" + "G" * 64):
            with patch.object(sys, "argv", ["gate", "--target", "server", "--digest", value, "--receipt", "unused.json"]), patch.object(policy, "request") as network:
                with self.assertRaises(ValueError): policy.main()
                network.assert_not_called()

    def test_manual_acl_review_is_bound_to_the_actual_package_ids(self):
        policy.validate_access_review("PRIVATE_ACCESS_REVIEWED:10:20", [10, 20])
        for value in ("", "true", "PRIVATE_ACCESS_REVIEWED:11:20"):
            with self.assertRaises(ValueError): policy.validate_access_review(value, [10, 20])

    def test_anonymous_registry_paths_keep_slashes_and_scope_is_encoded(self):
        digest = "sha256:" + "a" * 64
        with patch.object(policy, "request", side_effect=[(200, b'{"token":"synthetic-anonymous-capability"}'), (401, b'')]) as requests:
            policy.anonymous_denied("dal/server", digest)
            self.assertIn("scope=repository%3Afidan-lang%2Fdal%2Fserver%3Apull", requests.call_args_list[0].args[0])
            self.assertEqual(requests.call_args_list[1].args, ("https://ghcr.io/v2/fidan-lang/dal/server/manifests/" + digest, "synthetic-anonymous-capability"))
        with patch.object(policy, "request", side_effect=[(200, b'{"token":"synthetic-anonymous-capability"}'), (200, b'{}')]):
            with self.assertRaises(ValueError): policy.anonymous_denied("dal/worker", digest)

    def test_anonymous_pull_requires_denial_and_fails_on_network_errors(self):
        for status in (401, 403, 404): policy.validate_anonymous_status(status)
        for status in (200, 301, 429, 500):
            with self.assertRaises(ValueError): policy.validate_anonymous_status(status)


if __name__ == "__main__": unittest.main()
