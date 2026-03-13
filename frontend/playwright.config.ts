/// <reference types="node" />

import { defineConfig } from "@playwright/test";

const baseURL = process.env.DAL_WEB_BASE_URL ?? "http://localhost:4174";

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
    channel: process.env.CI ? undefined : "chrome",
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
