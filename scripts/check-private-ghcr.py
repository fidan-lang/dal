"""Read-only, fail-closed private GHCR release gate. Never creates or changes packages."""
import argparse
import json
import os
from pathlib import Path
import re
import urllib.error
import urllib.parse
import urllib.request

ORGANIZATION = "fidan-lang"
REPOSITORY = "fidan-lang/dal"
PACKAGES = ("dal/server", "dal/worker")
OWNER = "AppSolves"
ENVIRONMENT = "ghcr-release"


def validate_package(value, name):
    if value.get("name") != name or value.get("visibility") != "private" or type(value.get("id")) is not int:
        raise ValueError("Existing package identity and explicit PRIVATE visibility are required")
    return value["id"]


def validate_environment(value, branches):
    # Automated releases must never wait for human approval. All access is
    # scoped to the canonical protected main branch, and package visibility
    # is independently checked both before and after publication.
    rules = value.get("protection_rules", [])
    if value.get("name") != ENVIRONMENT or value.get("can_admins_bypass") is not False:
        raise ValueError("ghcr-release requires an explicit non-bypass environment")
    if any(r.get("type") == "required_reviewers" for r in rules):
        raise ValueError("Automatic GHCR releases require no required reviewers")
    if ((value.get("deployment_branch_policy") or {}).get("custom_branch_policies") is not True
            or (value.get("deployment_branch_policy") or {}).get("protected_branches") is not False):
        raise ValueError("Explicit main-only environment branch policy required")
    policies = branches.get("branch_policies", [])
    if len(policies) != 1 or policies[0].get("name") != "main" or policies[0].get("type") != "branch":
        raise ValueError("Release environment must allow only the main branch")


def validate_owner(membership, owners):
    if membership.get("role") != "admin" or membership.get("state") != "active":
        raise ValueError("AppSolves must be an active organization owner")
    if len(owners) != 1 or owners[0].get("login") != OWNER:
        raise ValueError("The release policy requires AppSolves as the sole organization owner")


def validate_access_review(review, ids):
    expected = "PRIVATE_ACCESS_REVIEWED:" + ":".join(map(str, ids))
    if review != expected:
        raise ValueError("Owner ACL/inheritance review must be explicitly recorded for both actual package IDs")


def validate_anonymous_status(status):
    if status not in (401, 403, 404):
        raise ValueError("Anonymous image access was not denied, or its denial could not be established")


class NoRedirect(urllib.request.HTTPRedirectHandler):
    def redirect_request(self, req, fp, code, msg, headers, newurl):
        return None


def request(url, token=None):
    headers = {"Accept": "application/json", "User-Agent": "dal-private-image-gate"}
    if token:
        if "\r" in token or "\n" in token:
            raise ValueError("Invalid authentication value")
        headers["Authorization"] = "Bearer " + token
    try:
        with urllib.request.build_opener(NoRedirect).open(urllib.request.Request(url, headers=headers), timeout=20) as response:
            payload = response.read(1024 * 1024 + 1)
            if len(payload) > 1024 * 1024:
                raise ValueError("Registry metadata exceeds gate limit")
            return response.status, payload
    except urllib.error.HTTPError as error:
        return error.code, b""
    except (urllib.error.URLError, OSError):
        raise ValueError("Registry metadata request failed; publication is blocked") from None


def api(path, token):
    status, body = request("https://api.github.com/" + path, token)
    if status != 200:
        raise ValueError(f"GitHub metadata unavailable (HTTP {status}); PRIVATE status/protection is unconfirmed")
    return json.loads(body)


def anonymous_denied(name, digest):
    scope = urllib.parse.urlencode({"service": "ghcr.io", "scope": f"repository:{ORGANIZATION}/{name}:pull"})
    status, payload = request("https://ghcr.io/token?" + scope)
    if status != 200:
        validate_anonymous_status(status)
        return
    token = json.loads(payload).get("token")
    if not isinstance(token, str) or not token:
        raise ValueError("Anonymous authentication response is invalid")
    status, _ = request(f"https://ghcr.io/v2/{ORGANIZATION}/{name}/manifests/{digest}", token)
    validate_anonymous_status(status)


def main():
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("--target", choices=("server", "worker"))
    parser.add_argument("--digest")
    parser.add_argument("--receipt", type=Path, required=True)
    options = parser.parse_args()
    if bool(options.target) != bool(options.digest) or (options.digest and not re.fullmatch(r"sha256:[a-f0-9]{64}", options.digest)):
        raise ValueError("An actual target and immutable image digest must be supplied together")
    if os.environ.get("GITHUB_REPOSITORY") != REPOSITORY or os.environ.get("GITHUB_REF") != "refs/heads/main":
        raise ValueError("Publication checks require the official repository main branch")
    token = os.environ.get("GH_TOKEN")
    if not token:
        raise ValueError("Read-only package/environment inspection credentials are required")
    for name in ("GHCR_PUBLISH_TOKEN", "GHCR_PULL_TOKEN"):
        if not os.environ.get(name):
            raise ValueError(f"Protected-environment credential missing: {name}")
    environment = api(f"repos/{REPOSITORY}/environments/{ENVIRONMENT}", token)
    branches = api(f"repos/{REPOSITORY}/environments/{ENVIRONMENT}/deployment-branch-policies", token)
    validate_environment(environment, branches)
    validate_owner(api(f"orgs/{ORGANIZATION}/memberships/{OWNER}", token),
                   api(f"orgs/{ORGANIZATION}/members?role=admin&per_page=2", token))
    packages = []
    for name in PACKAGES:
        base = f"orgs/{ORGANIZATION}/packages/container/{urllib.parse.quote(name, safe='')}"
        value = api(base, token)
        identity = validate_package(value, name)
        versions = api(base + "/versions?per_page=1", token)
        digest = options.digest if name == "dal/" + str(options.target) else (versions[0].get("name") if versions else None)
        if not isinstance(digest, str) or not re.fullmatch(r"sha256:[a-f0-9]{64}", digest):
            raise ValueError("An existing immutable package version is required before publication; no implicit package creation")
        anonymous_denied(name, digest)
        packages.append({"name": name, "id": identity, "visibility": "private", "digest": digest, "anonymous_pull": "denied"})
    # GitHub's package REST metadata does not expose inheritance or the complete ACL.
    # This explicit protected-environment attestation is an additional HUMAN gate,
    # never a claim that the REST visibility check proved those settings.
    validate_access_review(os.environ.get("GHCR_PRIVATE_ACCESS_REVIEW", ""), [p["id"] for p in packages])
    options.receipt.parent.mkdir(parents=True, exist_ok=True)
    options.receipt.write_text(json.dumps({"repository": REPOSITORY, "environment": ENVIRONMENT, "packages": packages, "acl_review": "owner-attested; not API-inferred"}, indent=2) + "\n", encoding="utf-8")
    print("Both existing packages are PRIVATE; anonymous access denied; protected release/owner ACL gates satisfied")


if __name__ == "__main__":
    try:
        main()
    except (ValueError, KeyError, TypeError, json.JSONDecodeError) as error:
        raise SystemExit(str(error)) from None
