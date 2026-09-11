import { defineConfig } from "@playwright/test";

export default defineConfig({
  testDir: "./tests/visual",
  timeout: 30_000,
  expect: { timeout: 8_000 },
  fullyParallel: false,
  workers: 1,
  reporter: [["line"]],
  use: {
    baseURL: "http://127.0.0.1:1431",
    browserName: "chromium",
    locale: "zh-CN",
    screenshot: "off",
    trace: "retain-on-failure",
  },
  webServer: {
    command: "vite --config frontend/vite.config.ts --port 1431",
    url: "http://127.0.0.1:1431",
    reuseExistingServer: false,
    timeout: 120_000,
  },
});
