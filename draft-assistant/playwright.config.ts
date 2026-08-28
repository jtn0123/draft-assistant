import { defineConfig, devices } from "@playwright/test";

/**
 * End-to-end tests against the browser-preview mode (`src/api.ts` falls back to
 * `public/dev-fixture.json` outside Tauri), driven by a real Chromium.
 *
 * This covers the rendered UI — layout, filtering, focus, live-region wiring —
 * in a real engine rather than jsdom. It deliberately does NOT cover the Tauri
 * IPC boundary, which the browser fallback stubs out; a true desktop E2E on
 * macOS would need WebdriverIO's embedded WebDriver server (`tauri-driver`
 * itself has no macOS WKWebView driver).
 */
export default defineConfig({
  testDir: "./e2e",
  fullyParallel: true,
  forbidOnly: !!process.env.CI,
  retries: process.env.CI ? 2 : 0,
  reporter: process.env.CI ? "list" : [["list"], ["html", { open: "never" }]],
  use: {
    baseURL: "http://localhost:1420",
    trace: "on-first-retry",
    screenshot: "only-on-failure",
  },
  projects: [
    { name: "chromium", use: { ...devices["Desktop Chrome"] } },
  ],
  webServer: {
    command: "bun run dev",
    url: "http://localhost:1420",
    reuseExistingServer: !process.env.CI,
    timeout: 60_000,
  },
});
