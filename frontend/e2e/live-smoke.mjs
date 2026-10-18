// Keep the legacy entry point, but use the maintained assertions and fresh local
// accounts instead of reading private credentials or printing session cookies.
import { execFileSync } from "node:child_process";
import { fileURLToPath } from "node:url";
import { localTestEndpoint } from "./local-test-endpoints.mjs";
for (const [key, fallback] of Object.entries({
  DAL_WEB_BASE_URL: "http://localhost:4174",
  DAL_API_BASE_URL: "http://localhost:18080",
  DAL_MAIL_SINK_URL: "http://127.0.0.1:8025",
})) {
  localTestEndpoint(process.env[key] ?? fallback, key);
}
execFileSync(
  process.execPath,
  [
    fileURLToPath(
      new URL("../node_modules/@playwright/test/cli.js", import.meta.url),
    ),
    "test",
    "release-smoke.spec.ts",
  ],
  {
    cwd: fileURLToPath(new URL("..", import.meta.url)),
    stdio: "inherit",
    env: process.env,
  },
);
