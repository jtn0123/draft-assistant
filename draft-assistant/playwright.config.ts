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

// Locally the dev server, for the edit-and-rerun loop. In CI (or with
// PW_WEB_SERVER=preview) the production bundle: `vite build` then `vite
// preview` on the same port, so the suite exercises the chunking, minified
// output and asset paths the .app ships, not the unbundled dev transform. A
// chunk that only breaks once built used to pass here and fail in the window.
const productionBundle = !!process.env.CI || process.env.PW_WEB_SERVER === "preview";

// 1420 is the port the Tauri dev server owns. PW_PORT moves the run when that
// port is busy (a `tauri dev` left running), read by `preview:e2e` too so the
// server and the tests agree.
const port = Number(process.env.PW_PORT ?? 1420);

export default defineConfig({
  testDir: "./e2e-browser",
  fullyParallel: true,
  forbidOnly: !!process.env.CI,
  // Never retried, in CI least of all: a test that passes on its second run
  // is a flaky test, and a retry is how one stays that way.
  retries: 0,
  reporter: process.env.CI ? "list" : [["list"], ["html", { open: "never" }]],
  outputDir: "./e2e-browser/.results",
  use: {
    baseURL: `http://localhost:${port}`,
    trace: "on-first-retry",
    screenshot: "only-on-failure",
  },
  projects: [{ name: "chromium", use: { ...devices["Desktop Chrome"] } }],
  webServer: {
    command: productionBundle
      ? "npm run build && npm run preview:e2e"
      : `npm run dev -- --port ${port}`,
    url: `http://localhost:${port}`,
    reuseExistingServer: !process.env.CI,
    // The production path pays for a build before the server answers.
    timeout: productionBundle ? 180_000 : 60_000,
  },
});
