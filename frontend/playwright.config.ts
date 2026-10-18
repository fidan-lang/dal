/// <reference types="node" />

import { defineConfig } from "@playwright/test";
import { localTestEndpoint } from "./e2e/local-test-endpoints.mjs";

const baseURL = localTestEndpoint(
  process.env.DAL_WEB_BASE_URL ?? "http://127.0.0.1:4174",
  "web",
);

export default defineConfig({
  testDir: "./e2e",
  fullyParallel: false,
  workers: 1,
  timeout: 60_000,
  expect: {
    timeout: 10_000,
  },
  use: {
    baseURL,
    headless: true,
    trace: "retain-on-failure",
    screenshot: "only-on-failure",
    video: "off",
    browserName: "chromium",
    // Use the browser revision pinned by package-lock, matching CI.
    // Explicit host-browser comparison remains available for investigation.
    channel: process.env.DAL_BROWSER_CHANNEL,
  },
  webServer: process.env.DAL_WEB_BASE_URL
    ? undefined
    : {
        command: "npm run build && npm run e2e:serve",
        port: 4174,
        reuseExistingServer: false,
        timeout: 180_000,
      },
  reporter: [["list"], ["html", { open: "never" }]],
});
