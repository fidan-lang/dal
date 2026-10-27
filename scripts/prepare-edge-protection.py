"""Generate reviewable Cloudflare rules from DAL policy. Never calls an API or deploys."""
import argparse
import json
from pathlib import Path
import re

ROOT = Path(__file__).resolve().parents[1]


def prepare(registry_host, existing=None):
    if not re.fullmatch(r"(?=.{1,253}$)(?:[a-z0-9](?:[a-z0-9-]{0,61}[a-z0-9])?\.)+[a-z][a-z0-9-]*", registry_host):
        raise ValueError("Registry host must be an exact lowercase DNS hostname, without scheme, port or expression")
    policy = json.loads((ROOT / "config/public-read-policy.json").read_text())
    host = json.dumps(registry_host)
    release = '(http.host eq "releases.fidan.dev" and (starts_with(http.request.uri.path, "/fidan/") or starts_with(http.request.uri.path, "/toolchains/")) and (ends_with(http.request.uri.path, ".tar.gz") or ends_with(http.request.uri.path, ".exe")))'
    download = f'(http.host eq {host} and starts_with(http.request.uri.path, "/packages/") and ends_with(http.request.uri.path, "/download"))'
    cache_key = {"custom_key": {"query_string": {"exclude": {"all": True}}}}
    download_scope = f"({release} or {download})"
    anonymous_read = '(http.request.method in {"GET" "HEAD"} and not any(http.request.headers["authorization"][*] ne "") and not any(http.request.headers["cookie"][*] ne ""))'
    release = f"({release} and {anonymous_read})"
    download = f"({download} and {anonymous_read})"
    errors = [{"status_code_range": {"from": 300, "to": 599}, "value": -1}]
    value = {
        "status": "REVIEW_ONLY_NOT_APPLIED",
        "preserve_existing_rules": True,
        "cache_rules": [
            {"ref": "fidan_immutable_releases", "description": "Cache public immutable release archives, ignoring cache-busting query strings", "enabled": True,
             "expression": release, "action": "set_cache_settings", "action_parameters": {
                 "cache": True, "cache_key": cache_key,
                 "edge_ttl": {"mode": "override_origin", "default": policy["archive_cache_seconds"],
                              "status_code_ttl": errors}}},
            {"ref": "dal_immutable_downloads", "description": "Cache public streamed packages; respect legacy no-store and error responses", "enabled": True,
             "expression": download, "action": "set_cache_settings", "action_parameters": {
                 "cache": True, "cache_key": cache_key, "edge_ttl": {"mode": "bypass_by_default", "status_code_ttl": errors}}},
        ],
        "additional_rate_rule": {
            "ref": "fidan_public_download_quota", "description": "Limit anonymous archive requests without browser challenges", "enabled": True,
            "expression": download_scope, "action": "block",
            "ratelimit": {"characteristics": ["cf.colo.id", "ip.src"], "period": 60,
                          "requests_per_period": policy["download_requests_per_minute"], "mitigation_timeout": 60},
        },
        "rate_rule_gate": "Requires an additional rule and hostname/60-second support (Pro or higher); preserve the existing leaked-credential rule. No automatic plan upgrade.",
        "origin_gate": "Keep container ports loopback-only; verify the actual reverse proxy/tunnel denies direct-origin access and overwrites one trusted client-IP header before configuring DAL_TRUSTED_PROXIES.",
    }
    # Produce additions only from a freshly exported entrypoint. Never replace
    # unrelated rules, and stop if an owned ref has drifted rather than overwrite it.
    value["cache_rules_to_add"] = None
    if existing is not None:
        if existing.get("phase") != "http_request_cache_settings" or not isinstance(existing.get("rules"), list):
            raise ValueError("Expected an exported cache-settings entrypoint")
        indexed = {}
        for rule in existing["rules"]:
            ref = rule.get("ref")
            if not isinstance(ref, str) or ref in indexed:
                raise ValueError("Existing rules require unique reference identifiers")
            indexed[ref] = rule
        additions = []
        for rule in value["cache_rules"]:
            prior = indexed.get(rule["ref"])
            if prior is None:
                additions.append(rule)
            elif any(prior.get(key) != expected for key, expected in rule.items()):
                raise ValueError("Owned cache rule differs; explicit review required before changing it")
        value["cache_rules_to_add"] = additions
    return value


if __name__ == "__main__":
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("--registry-host", required=True)
    parser.add_argument("--output", type=Path, required=True)
    parser.add_argument("--existing-cache-rules", type=Path, help="Freshly exported cache entrypoint; compare refs without overwriting existing rules")
    options = parser.parse_args()
    existing = json.loads(options.existing_cache_rules.read_text()) if options.existing_cache_rules else None
    rules = prepare(options.registry_host, existing)
    options.output.parent.mkdir(parents=True, exist_ok=True)
    options.output.write_text(json.dumps(rules, indent=2) + "\n")
    print("Prepared rules only; nothing deployed. Review plan quota, origin and private bucket access first.")
