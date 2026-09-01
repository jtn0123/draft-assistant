import { defineConfig, devices } from "@playwright/test";

/**
 * Browser end-to-end tests against the preview mode: outside Tauri, `api.ts`
 * serves the checked-in dumps (`public/dev-fixture.json`,
 * `public/dev-season-fixture.json`), so a real Chromium can render the whole
 * app with a real, fixed league.
 *
 * What this covers that jsdom cannot: layout, overflow, focus order, and the
 * tab/roving-tabindex behaviour of the season rail. What it deliberately does
 * NOT cover is the Tauri IPC boundary, which the preview stubs out —
 * `npm run test:e2e` drives the real desktop window through WebdriverIO for
 * that, and is a separate, much heavier package.
 */
export default defineConfig({
  testDir: "./e2e-browser",
  fullyParallel: true,
  forbidOnly: !!process.env.CI,
  retries: process.env.CI ? 2 : 0,
  reporter: process.env.CI ? "list" : [["list"], ["html", { open: "never" }]],
  outputDir: "./e2e-browser/.results",
  use: {
    baseURL: "http://localhost:1420",
    trace: "on-first-retry",
    screenshot: "only-on-failure",
  },
  projects: [{ name: "chromium", use: { ...devices["Desktop Chrome"] } }],
  webServer: {
    command: "npm run dev",
    url: "http://localhost:1420",
    reuseExistingServer: !process.env.CI,
    timeout: 60_000,
  },
});
