"""Exercise the actual Linux CI collector with jq; no network or real credentials."""
from pathlib import Path
import os
import shutil
import subprocess
import tempfile

ROOT = Path(__file__).resolve().parents[1]
source = (ROOT / ".github/workflows/ecosystem-acceptance.yaml").read_text()
start = source.index("          collect_audit() {")
end = source.index("          collect_audit fidan", start)
function = "\n".join(line[10:] for line in source[start:end].splitlines())
checks = r'''
set -euo pipefail
fixture() { printf '%s' "$1"; return "$2"; }
collect_audit . clean.json rust fixture '{"vulnerabilities":{"found":false,"count":0}}' 0
collect_audit . finding.json rust fixture '{"vulnerabilities":{"found":true,"count":1}}' 1
collect_audit . npm.json npm fixture '{"metadata":{"vulnerabilities":{"total":0}}}' 0
set +e
( set -e; collect_audit . malformed.json rust fixture '{"error":"network"}' 1 )
bad_schema=$?
( set -e; collect_audit . tool.json rust fixture '{}' 2 )
bad_tool=$?
set -e
[ "$bad_schema" -ne 0 ]
[ "$bad_tool" -ne 0 ]
echo 'Actual workflow audit collector: 5 cases passed'
'''
bash = shutil.which("bash")
if os.name == "nt":
    git = Path(shutil.which("git") or "")
    candidate = git.parent.parent / "bin/bash.exe"
    if candidate.is_file():
        bash = str(candidate)
if not bash:
    raise SystemExit("Bash and jq are required for this Linux workflow regression")
(ROOT / "target").mkdir(exist_ok=True)
with tempfile.TemporaryDirectory(prefix="audit-collector-", dir=ROOT / "target") as temp:
    script = Path(temp) / "check.sh"
    script.write_text(function + "\n" + checks, encoding="utf-8")
    subprocess.run([bash, str(script)], cwd=temp, check=True, timeout=30)
