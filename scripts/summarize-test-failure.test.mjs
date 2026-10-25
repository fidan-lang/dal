import test from "node:test";
import assert from "node:assert/strict";
import { summarize } from "./summarize-test-failure.mjs";
test("failed-test summaries print only known source function names, never captured payloads", () => {
  const log = "test auth::login ... FAILED\nthread 'login' panicked: token=synthetic-secret\nleft: eyJ.synthetic.jwt\n-----BEGIN PRIVATE KEY-----\nprivate-value\nhttps://user:password@example.test\ntest synthetic_secret ... FAILED\n";
  assert.equal(summarize(log, new Set(["login"])), "Rust test failed: login");
  assert(!summarize("token=synthetic-secret", new Set()).includes("synthetic-secret"));
});

test("panic locations are limited to known source files and real line ranges", () => {
  const log = "thread 'auth::login' (123) panicked at crates/auth/tests.rs:10:2:\nsecret message\nthread 'auth::login' panicked at crates/credential.rs:1:1:\nthread 'auth::login' panicked at crates/auth/tests.rs:999999999:1:\n";
  assert.equal(summarize(log, new Set(["login"]), new Map([["crates/auth/tests.rs", 20]])), "Rust panic location: crates/auth/tests.rs:10");
});

test("storage diagnostics expose only closed categories", () => {
  assert.equal(summarize('Storage("object upload failed [missing_bucket]")\nobject upload failed [synthetic-secret]\n', new Set()), "Storage upload category: missing_bucket");
});

// The acceptance runner must also report compiler failures with the same safe parser.
test("CLI can summarize another exact source checkout without printing its private log", async () => {
  const { mkdtempSync, mkdirSync, writeFileSync, rmSync } = await import("node:fs");
  const { fileURLToPath } = await import("node:url");
  const { spawnSync } = await import("node:child_process");
  const path = await import("node:path");
  const target = fileURLToPath(new URL("../target/", import.meta.url));
  mkdirSync(target, { recursive: true });
  const scratch = mkdtempSync(path.join(target, "failure-summary-"));
  try {
    mkdirSync(path.join(scratch, "crates"));
    writeFileSync(path.join(scratch, "crates/probe.rs"), "fn known_failure() {}\n");
    const log = path.join(scratch, "private.log");
    writeFileSync(log, "test module::known_failure ... FAILED\nthread 'module::known_failure' panicked at crates/probe.rs:1:1:\nAuthorization: synthetic-secret\nhttps://user:password@example.test\n");
    const result = spawnSync(process.execPath, [fileURLToPath(new URL("./summarize-test-failure.mjs", import.meta.url)), log, scratch], { encoding: "utf8" });
    assert.equal(result.status, 0, result.stderr);
    assert.equal(result.stdout.trim(), "Rust test failed: known_failure\nRust panic location: crates/probe.rs:1");
    assert(!result.stdout.includes("synthetic-secret"));
    assert(!result.stdout.includes("password"));
  } finally { rmSync(scratch, { recursive: true, force: true }); }
});
