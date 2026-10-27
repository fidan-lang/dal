"""Generate reviewable Cloudflare rules from DAL policy. Never calls an API or deploys."""
import argparse
import json
from pathlib import Path
import re

ROOT = Path(__file__).resolve().parents[1]


def prepare(registry_host):
    if not re.fullmatch(r"(?=.{1,253}$)(?:[a-z0-9](?:[a-z0-9-]{0,61}[a-z0-9])?\.)+[a-z][a-z0-9-]*", registry_host):
        raise ValueError("Registry host must be an exact lowercase DNS hostname, without scheme, port or expression")
    policy = json.loads((ROOT / "config/public-read-policy.json").read_text())
    host = json.dumps(registry_host)
    release = '(http.host eq "releases.fidan.dev" and (starts_with(http.request.uri.path, "/fidan/") or starts_with(http.request.uri.path, "/toolchains/")) and (ends_with(http.request.uri.path, ".tar.gz") or ends_with(http.request.uri.path, ".exe")))'
    download = f'(http.host eq {host} and starts_with(http.request.uri.path, "/packages/") and ends_with(http.request.uri.path, "/download"))'
    cache_key = {"custom_key": {"query_string": {"exclude": {"all": True}}}}
    return {
        "status": "REVIEW_ONLY_NOT_APPLIED",
        "preserve_existing_rules": True,
        "cache_rules": [
            {"ref": "fidan_immutable_releases", "description": "Cache public immutable release archives, ignoring cache-busting query strings", "enabled": True,
             "expression": release, "action": "set_cache_settings", "action_parameters": {
                 "cache": True, "cache_key": cache_key,
                 "edge_ttl": {"mode": "override_origin", "default": policy["archive_cache_seconds"],
                              "status_code_ttl": [{"status_code_range": {"from": 300, "to": 599}, "value": -1}]}}},
            {"ref": "dal_immutable_downloads", "description": "Cache public streamed packages; respect legacy no-store and error responses", "enabled": True,
             "expression": download, "action": "set_cache_settings", "action_parameters": {
                 "cache": True, "cache_key": cache_key, "edge_ttl": {"mode": "bypass_by_default"}}},
        ],
        "additional_rate_rule": {
            "ref": "fidan_public_download_quota", "description": "Limit anonymous archive requests without browser challenges", "enabled": True,
            "expression": f"({release} or {download})", "action": "block",
            "ratelimit": {"characteristics": ["cf.colo.id", "ip.src"], "period": 60,
                          "requests_per_period": policy["download_requests_per_minute"], "mitigation_timeout": 60},
        },
        "rate_rule_gate": "Requires an additional rule and hostname/60-second support (Pro or higher); preserve the existing leaked-credential rule. No automatic plan upgrade.",
        "origin_gate": "Keep container ports loopback-only; verify the actual reverse proxy/tunnel denies direct-origin access and overwrites one trusted client-IP header before configuring DAL_TRUSTED_PROXIES.",
    }


if __name__ == "__main__":
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("--registry-host", required=True)
    parser.add_argument("--output", type=Path, required=True)
    options = parser.parse_args()
    rules = prepare(options.registry_host)
    options.output.parent.mkdir(parents=True, exist_ok=True)
    options.output.write_text(json.dumps(rules, indent=2) + "\n")
    print("Prepared rules only; nothing deployed. Review plan quota, origin and private bucket access first.")
