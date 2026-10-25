"""Execute only the six-repository candidate-input guard, with synthetic refs."""
import json
import os
from pathlib import Path
import shutil
import subprocess
import tempfile
import unittest
import yaml

ROOT = Path(__file__).resolve().parents[1]

class CandidateInputTests(unittest.TestCase):
    def test_localstack_ready_hook_is_executable_in_git_and_fails_closed(self):
        staged = subprocess.run(["git", "ls-files", "--stage", "--", "scripts/localstack-init-test.sh"], cwd=ROOT, check=True, capture_output=True, text=True)
        self.assertTrue(staged.stdout.startswith("100755 "), "LocalStack ignores non-executable init hooks")
        self.assertNotIn("|| true", (ROOT / "scripts/localstack-init-test.sh").read_text(encoding="utf-8"))

    def test_exact_six_sha_and_official_repository_contract(self):
        workflow = yaml.safe_load((ROOT / ".github/workflows/ecosystem-acceptance.yaml").read_text(encoding="utf-8"))
        guard = next(s for s in workflow["jobs"]["acceptance"]["steps"] if s.get("id") == "refs")
        names = ("fidan", "dal", "fidan-actions", "fidan-editors", "fidan.dev", "args")
        valid = dict.fromkeys(names, "a" * 40)
        cases = [(valid, "fidan-lang/args", True)]
        for value in ("main", "a" * 40 + "\n", "a" * 40 + "\nFORGED=1", '"; throw "INJECTED', 123, None):
            cases.append(({**valid, "fidan": value}, "fidan-lang/args", False))
        cases += [(valid, "fidan-lang/args\n", False), (valid, "someone/args", False)]
        (ROOT / "target").mkdir(exist_ok=True)
        with tempfile.TemporaryDirectory(dir=ROOT / "target", prefix="candidate-guard-") as tmp:
            scratch = Path(tmp)
            script = scratch / "guard.ps1"
            script.write_text("$ErrorActionPreference = 'Stop'\n" + guard["run"], encoding="utf-8")
            output = scratch / "output"
            for refs, repository, accepted in cases:
                with self.subTest(ref=refs["fidan"], repository=repository):
                    output.unlink(missing_ok=True)
                    env = {name: os.environ[name] for name in ("PATH", "SystemRoot", "WINDIR") if name in os.environ}
                    env.update(CANDIDATE_REFS=json.dumps(refs), ARGS_REPOSITORY=repository, GITHUB_OUTPUT=str(output))
                    # A credential-free shell also needs its own writable home/temp:
                    # avoid host profile/cache discovery and first-run telemetry.
                    home = scratch / "home"
                    home.mkdir(exist_ok=True)
                    env.update(HOME=str(home), USERPROFILE=str(home), TEMP=str(scratch), TMP=str(scratch), TMPDIR=str(scratch),
                               POWERSHELL_TELEMETRY_OPTOUT="1", DOTNET_CLI_TELEMETRY_OPTOUT="1")
                    result = subprocess.run([shutil.which("pwsh"), "-NoProfile", "-NonInteractive", "-File", str(script)], env=env,
                                            cwd=ROOT, stdin=subprocess.DEVNULL, capture_output=True, text=True, encoding="utf-8", timeout=20)
                    self.assertEqual(result.returncode == 0, accepted, result.stderr)
                    if accepted:
                        self.assertEqual(len(output.read_text().splitlines()), 6)
                    else:
                        self.assertFalse(output.exists(), "Invalid candidate wrote partial workflow outputs")

if __name__ == "__main__":
    unittest.main()
