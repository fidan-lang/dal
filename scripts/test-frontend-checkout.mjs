import assert from "node:assert/strict";
import { execFileSync, spawnSync } from "node:child_process";
import { mkdtempSync, mkdirSync, rmSync } from "node:fs";
import path from "node:path";
import { fileURLToPath } from "node:url";

const root = fileURLToPath(new URL("../", import.meta.url));
const target = path.join(root, "target");
mkdirSync(target, { recursive: true });
const scratch = mkdtempSync(path.join(target, "frontend-checkout-"));
assert.equal(path.dirname(scratch), target);
try {
  execFileSync(
    "git",
    [
      "-c",
      "core.autocrlf=true",
      "checkout-index",
      `--prefix=${scratch.replaceAll("\\", "/")}/`,
      "--all",
    ],
    { cwd: root },
  );
  const checked = spawnSync(
    process.execPath,
    [
      path.join(root, "frontend/node_modules/prettier/bin/prettier.cjs"),
      "--check",
      ".",
    ],
    { cwd: path.join(scratch, "frontend"), encoding: "utf8" },
  );
  assert.equal(
    checked.status,
    0,
    `Windows checkout must pass the real frontend formatter:\n${checked.stdout}\n${checked.stderr}`,
  );
} finally {
  rmSync(scratch, { recursive: true, force: true });
}
console.log("Fresh Windows checkout frontend formatting passed.");
