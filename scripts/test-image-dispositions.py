"""Offline regression checks for honest, complete CVE disposition evidence."""
import importlib.util
from pathlib import Path
import unittest
import sys
sys.dont_write_bytecode = True
spec = importlib.util.spec_from_file_location("dispositions", Path(__file__).with_name("disposition-images.py"))
module = importlib.util.module_from_spec(spec)
spec.loader.exec_module(module)

class Dispositions(unittest.TestCase):
    def test_slim_documentation_exclusion_never_hides_binary_changes(self):
        self.assertTrue(module.only_missing_documentation(""))
        self.assertTrue(module.only_missing_documentation("missing     /usr/share/doc/gzip/README.gz"))
        for line in ("missing /usr/bin/gzip", "??5?????? /usr/bin/gzip", "missing /etc/security.conf", "unexpected error"):
            self.assertFalse(module.only_missing_documentation(line))

    def test_backport_rejects_modified_or_older_package(self):
        from unittest.mock import patch
        from subprocess import CompletedProcess
        artifact = {"type": "binary", "name": "gzip", "metadata": {"matches": [{"location": {"path": "/usr/bin/gzip"}}]}}
        sbom = {"artifacts": [{"type": "deb", "name": "gzip", "version": "1.13-1+deb13u1"}]}
        owner = CompletedProcess([], 0, "gzip: /usr/bin/gzip\n", "")
        good = CompletedProcess([], 0, "", "")
        changed = CompletedProcess([], 0, "??5?????? /usr/bin/gzip\n", "")
        older = CompletedProcess([], 1, "", "")
        image = "sha256:" + "a" * 64
        for integrity, comparison, expected in [(good, good, "1.13-1+deb13u1"), (changed, good, None), (good, older, None)]:
            with patch.object(module.subprocess, "run", side_effect=[owner, integrity, comparison]) as run:
                self.assertEqual(module.verified_backport(artifact, sbom, image, "1.13-1+deb13u1"), expected)
                self.assertIn("--read-only", run.call_args.args[0])
                self.assertIn("none", run.call_args.args[0])

    def test_identity_vendor_and_unknown_findings_fail_closed(self):
        sbom = {"source": {"metadata": {"imageID": "sha256:exact"}}, "artifacts": [
            {"id": "a", "name": "libc6", "version": "2.41", "metadata": {"source": "glibc"}}]}
        match = {"artifact": {"id": "a"}, "vulnerability": {"id": "CVE-fixture", "severity": "High"}}
        scan = {"matches": [match, match]}
        with self.assertRaises(ValueError): module.reconcile(sbom, scan, {}, "sha256:other")
        row, = module.reconcile(sbom, scan, {}, "sha256:exact")
        self.assertEqual(row["scanner_matches"], 2)
        self.assertTrue(row["disposition"].startswith("PRODUCTION_BLOCKED"))
        for fixed in (None, "2.42", "0"):
            tracker = {"glibc": {"CVE-fixture": {"releases": {"trixie": {"status": "resolved", "fixed_version": fixed}}}}}
            row, = module.reconcile(sbom, scan, tracker, "sha256:exact")
            self.assertEqual(row["disposition"] == "VENDOR_NOT_AFFECTED", fixed == "0")
        tracker = {"glibc": {"CVE-fixture": {"releases": {"trixie": {"status": "resolved", "fixed_version": "2.42"}}}}}
        row, = module.reconcile(sbom, scan, tracker, "sha256:exact", lambda *args: "2.42+deb13u1")
        self.assertEqual(row["disposition"], "VENDOR_BACKPORT_VERIFIED")
        self.assertIsNone(module.verified_backport({"type": "binary"}, {}, "moving:tag", "1"))
        with self.assertRaises(KeyError): module.reconcile({**sbom, "artifacts": []}, scan, {}, "sha256:exact")

if __name__ == "__main__": unittest.main()
