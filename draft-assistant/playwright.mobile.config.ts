import { defineConfig, devices } from "@playwright/test";

/**
 * The phone page in the two browsers a phone actually runs: WebKit as an
 * iPhone and Chromium as an Android phone, light and dark. Nothing here needs
 * the app's dev server: `companionServer.ts` serves the page and answers as
 * the host, so the base address only has to be one nothing listens on.
 *
 * Service workers are blocked on purpose. Chromium treats localhost as a
 * secure context, registers the page's worker, and a request the worker
 * makes goes around Playwright's routing to the real network, which is the
 * running desktop app when it is on the port. `npm run test:e2e:mobile`.
 */
export default defineConfig({
  testDir: "./e2e-browser",
  testMatch: /companion-mobile\.spec\.ts/,
  retries: 0,
  reporter: [["list"]],
  outputDir: "./e2e-browser/.results-mobile",
  use: { baseURL: "http://localhost:7999", serviceWorkers: "block", trace: "off" },
  projects: [
    { name: "iphone-webkit", use: { ...devices["iPhone 15"] } },
    { name: "android-chromium", use: { ...devices["Pixel 7"] } },
  ],
});
