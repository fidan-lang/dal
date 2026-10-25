import test from "node:test";
import assert from "node:assert/strict";
import { summarize } from "./summarize-test-failure.mjs";
test("failed-test summaries print only known source function names, never captured payloads", () => {
  const log = "test auth::login ... FAILED\nthread 'login' panicked: token=synthetic-secret\nleft: eyJ.synthetic.jwt\n-----BEGIN PRIVATE KEY-----\nprivate-value\nhttps://user:password@example.test\ntest synthetic_secret ... FAILED\n";
  assert.equal(summarize(log, new Set(["login"])), "Rust test failed: login");
  assert(!summarize("token=synthetic-secret", new Set()).includes("synthetic-secret"));
});
