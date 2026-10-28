"""Reconcile every raw Grype match with its Syft source package and Debian tracker.

Produces evidence, not an automatic production risk acceptance. Reports stay local.
"""
import argparse
from collections import Counter
import hashlib
import json
import re
import subprocess
from pathlib import Path

TRACKER_URL = "https://security-tracker.debian.org/tracker/data/json"
# Actual Rust executables use Rustls/AWS-LC and don't launch these OS utilities.
# Installed packages still remain in the SBOM and require explicit disposition.
UTILITY_SOURCES = {"acl", "apt", "attr", "bzip2", "gzip", "ncurses", "pam",
                   "perl", "shadow", "sqlite3", "systemd", "util-linux", "zlib",
                   "coreutils", "dash", "diffutils", "tar"}


def only_missing_documentation(output):
    return all(re.fullmatch(r"missing\s+/usr/share/(?:doc|man|info)/\S+", line)
               for line in output.splitlines() if line.strip())


def verified_backport(artifact, sbom, image, fixed):
    """Check CPE binary matches against the owning Debian package in the exact image."""
    if artifact["type"] != "binary" or not re.fullmatch(r"sha256:[a-f0-9]{64}", image):
        return None
    packages = [a for a in sbom["artifacts"] if a["type"] == "deb" and a["name"] == artifact["name"]]
    matches = (artifact.get("metadata") or {}).get("matches", [])
    if len(packages) != 1 or not matches:
        return None
    package = packages[0]
    filename = matches[0]["location"]["path"]
    if not filename.startswith("/") or ".." in filename.split("/"):
        return None
    def run(command, *arguments):
        return subprocess.run(["docker", "run", "--rm", "--network", "none", "--read-only",
                               "--cap-drop", "ALL", "--security-opt", "no-new-privileges:true",
                               "--entrypoint", command, image, *arguments],
                              capture_output=True, text=True, timeout=30, check=False)
    owner = run("dpkg-query", "--search", filename)
    if owner.returncode != 0 or owner.stdout.strip() != package["name"] + ": " + filename:
        return None
    integrity = run("dpkg", "--verify", package["name"])
    comparison = run("dpkg", "--compare-versions", package["version"], "ge", fixed)
    # Official slim images omit manuals; any changed executable or other missing
    # path still fails verification. Do not ignore arbitrary dpkg diagnostics.
    if integrity.returncode == comparison.returncode == 0 and only_missing_documentation(integrity.stdout):
        return package["version"]
    return None


def reconcile(sbom, scan, tracker, identity, backport_check=None):
    image_id = sbom["source"]["metadata"]["imageID"]
    if image_id != identity:
        raise ValueError("SBOM/image identity mismatch")
    artifacts = {item["id"]: item for item in sbom["artifacts"]}
    rows = {}
    for match in scan["matches"]:
        finding = match["vulnerability"]
        artifact = artifacts[match["artifact"]["id"]]
        metadata = artifact.get("metadata") or {}
        source = metadata.get("source") or artifact["name"]
        key = (finding["id"], artifact["name"], artifact["version"])
        if key in rows:
            rows[key]["scanner_matches"] += 1
            continue
        vendor = tracker.get(source, {}).get(finding["id"], {})
        release = vendor.get("releases", {}).get("trixie", {})
        # Debian's fixed_version=0 is an explicit not-affected status. A nonzero
        # fix needs a real dpkg version comparison; never infer it from SemVer.
        unaffected = release.get("status") == "resolved" and release.get("fixed_version") == "0"
        if source in {"glibc", "gcc-14"}:
            exposure = "Runtime-library source: inspect the affected function against captured ldd output; network exploitability is not established."
        elif source in UTILITY_SOURCES:
            exposure = "Installed OS tooling/library: DAL does not spawn OS package utilities; non-root, read-only, dropped capabilities and no-new-privileges mitigate local paths, not every possible exploit."
        else:
            exposure = "Unclassified dependency; manual reachability review required before production."
        backport = None
        fixed = release.get("fixed_version")
        if backport_check and release.get("status") == "resolved" and fixed and fixed != "0":
            backport = backport_check(artifact, sbom, fixed)
        rows[key] = {
            "id": finding["id"], "severity": finding["severity"],
            "package": artifact["name"], "version": artifact["version"],
            "source_package": source, "source_version": metadata.get("sourceVersion"),
            "scanner_matches": 1, "verified_debian_backport": backport, "scanner_fix": finding.get("fix", {}),
            "scanner_url": finding.get("dataSource"),
            "description": finding.get("description") or vendor.get("description", ""),
            "debian_trixie": release, "debian_url": "https://security-tracker.debian.org/tracker/" + finding["id"],
            "runtime_assessment": exposure,
            "disposition": ("VENDOR_NOT_AFFECTED" if unaffected else "VENDOR_BACKPORT_VERIFIED" if backport
                            else "PRODUCTION_BLOCKED_PENDING_FIX_OR_EXPLICIT_RISK_ACCEPTANCE"),
        }
    return sorted(rows.values(), key=lambda row: (row["id"], row["package"]))


def attach_runtime_evidence(rows, runtime):
    affected = {
        "CVE-2026-19499": {"strfmon", "strfmon_l"},
        "CVE-2026-5435": {"ns_printrrf", "ns_printrr", "fp_nquery"},
    }
    imports = set(runtime["dynamicImports"])
    if not imports or runtime.get("privilegeBearingFiles") != 0:
        raise ValueError("Complete native import and privilege-bit evidence required")
    for row in rows:
        if row["id"] in affected:
            row["native_evidence"] = {
                "image_id": runtime["image"], "binary_sha256": runtime["binarySha256"],
                "directly_imported_affected_symbols": sorted(affected[row["id"]] & imports),
                "assessment": "No direct import does not exclude indirect libc reachability and is not risk acceptance.",
            }


def main():
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("--input", type=Path, required=True)
    parser.add_argument("--debian-tracker", type=Path, required=True)
    parser.add_argument("--runtime-evidence", type=Path)
    options = parser.parse_args()
    runtime = json.loads(options.runtime_evidence.read_text(encoding="utf-8")) if options.runtime_evidence else []
    tracker_bytes = options.debian_tracker.read_bytes()
    tracker = json.loads(tracker_bytes)
    identities = json.loads((options.input / "identities.json").read_text(encoding="utf-8-sig"))
    for identity in identities:
        name = identity["name"]
        if name not in {"server", "worker"}:
            raise ValueError("Unexpected image target")
        sbom = json.loads((options.input / f"{name}.sbom.json").read_text(encoding="utf-8-sig"))
        scan = json.loads((options.input / f"{name}.cves.json").read_text(encoding="utf-8-sig"))
        rows = reconcile(sbom, scan, tracker, identity["config_id"],
                         lambda artifact, inventory, fixed: verified_backport(artifact, inventory, identity["image_id"], fixed))
        if options.runtime_evidence:
            evidence = next((row for row in runtime if row["image"] == identity["image_id"] and row["kind"] == name), None)
            if evidence is None:
                raise ValueError("Runtime receipt must match the exact scanned image")
            attach_runtime_evidence(rows, evidence)
        output = {"image": identity, "tracker_url": TRACKER_URL,
                  "tracker_sha256": hashlib.sha256(tracker_bytes).hexdigest(),
                  "raw_severity_counts": dict(Counter(m["vulnerability"]["severity"] for m in scan["matches"])),
                  "distinct_ids": len({r["id"] for r in rows}),
                  "package_vulnerability_pairs": len(rows),
                  "production_approved": False, "findings": rows}
        (options.input / f"{name}.disposition.json").write_text(json.dumps(output, indent=2) + "\n", encoding="utf-8")
        print(name, output["raw_severity_counts"], "distinct IDs", output["distinct_ids"], "production NOT approved")


if __name__ == "__main__":
    main()
