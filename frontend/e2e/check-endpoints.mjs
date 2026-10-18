import assert from "node:assert/strict";
import { localTestEndpoint } from "./local-test-endpoints.mjs";
for (const url of [
  "http://127.0.0.1:18080",
  "http://localhost:4174",
  "http://[::1]:9229",
]) {
  assert.equal(localTestEndpoint(url, "test"), url);
}
for (const url of [
  "https://api.dal.fidan.dev",
  "https://dal.fidan.dev",
  "http://127.0.0.1.attacker.test",
  "http://user:secret@localhost",
  "file:///tmp/test",
]) {
  assert.throws(() => localTestEndpoint(url, "test"), /loopback test URL/);
}
console.log("Local-only E2E endpoint safety checks passed.");
