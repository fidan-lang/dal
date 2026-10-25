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
