"""Check active first-party license contracts; exclude dependency and audit history files."""
import hashlib
import json
from pathlib import Path
import re
import sys
import subprocess
import tomllib

APACHE_2_SHA256 = "cfc7749b96f63bd31c3c42b5c471bf756814053e847c10f3eb003417bc523d30"
OBSOLETE_POLICY = re.compile(
    r"Apache.{0,40}(?:Additional Terms|Modified)|LicenseRef-Fidan|"
    r"cannot commercially redistribute|may not be sold or marketed as a standalone",
    re.IGNORECASE,
)


def check(root):
    root = root.resolve()
    assert hashlib.sha256((root / "LICENSE").read_bytes()).hexdigest() == APACHE_2_SHA256, f"{root.name}: LICENSE must be unmodified Apache 2.0"
    assert "Copyright" in (root / "NOTICE").read_text(encoding="utf-8"), f"{root.name}: attribution NOTICE required"
    for relative in ("README.md", "CONTRIBUTING.md", "vscode/README.md"):
        file = root / relative
        if file.exists():
            assert not OBSOLETE_POLICY.search(file.read_text(encoding="utf-8")), f"{file}: obsolete active license policy"
    for relative in ("package.json", "frontend/package.json", "vscode/package.json"):
        file = root / relative
        if file.exists():
            assert json.loads(file.read_text(encoding="utf-8"))["license"] == "Apache-2.0", f"{file}: SPDX identifier required"
    web_root = root / "frontend" if (root / "frontend/package.json").exists() else root
    if (web_root / "static").exists():
        for name in ("LICENSE", "NOTICE"):
            assert (web_root / "static" / f"{name}.txt").read_bytes() == (root / name).read_bytes(), f"{root.name}: web legal artifact drift"
    if (root / "vscode/package.json").exists():
        assert (root / "vscode/LICENSE").read_bytes() == (root / "LICENSE").read_bytes(), "VSIX source license drift"
        assert (root / "vscode/NOTICE").read_bytes() == (root / "NOTICE").read_bytes(), "VSIX source NOTICE drift"
    if (root / "Cargo.toml").exists():
        metadata = json.loads(subprocess.check_output(["cargo", "metadata", "--no-deps", "--format-version", "1", "--locked"], cwd=root, text=True))
        members = set(metadata["workspace_members"])
        for package in metadata["packages"]:
            if package["id"] not in members:
                continue
            assert package["license"] == "Apache-2.0", f"{package['name']}: SPDX identifier required"
            license_copy = Path(package["manifest_path"]).parent / "LICENSE"
            assert license_copy.read_bytes() == (root / "LICENSE").read_bytes(), f"{license_copy}: crate license drift"
            notice = Path(package["manifest_path"]).parent / "NOTICE"
            assert notice.read_bytes() == (root / "NOTICE").read_bytes(), f"{notice}: crate must preserve root NOTICE"
    if (root / "dal.toml").exists():
        manifest = tomllib.loads((root / "dal.toml").read_text(encoding="utf-8"))
        assert manifest["package"]["license"] == "Apache-2.0", "Package SPDX identifier required"
        assert "NOTICE" in manifest["package"]["include"], "Package must preserve attribution"
    print(f"PASS {root.name}: Apache 2.0, attribution and active metadata")


if __name__ == "__main__":
    failed = []
    for argument in sys.argv[1:] or ["."]:
        try:
            check(Path(argument))
        except (AssertionError, KeyError, OSError, ValueError, subprocess.CalledProcessError) as error:
            failed.append(str(error))
    if failed:
        print("\n".join(failed), file=sys.stderr)
        sys.exit(1)
