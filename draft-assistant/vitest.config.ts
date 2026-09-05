import react from "@vitejs/plugin-react";
import { defineConfig } from "vitest/config";

export default defineConfig({
  plugins: [react()],
  test: {
    environment: "jsdom",
    globals: true,
    // The unit suite is the app's own; `e2e-browser/` belongs to Playwright,
    // whose `test.beforeEach` throws if vitest tries to collect it.
    include: ["src/**/*.{test,spec}.{ts,tsx}"],
    setupFiles: "./src/test/setup.ts",
    css: true,
    coverage: {
      provider: "v8",
      // The floor, not the goal — raise these as coverage climbs. Enforced by
      // `npm run test:coverage`, which `npm run verify` (and therefore CI) runs.
      //
      // Set roughly five points under what the suite actually covers, so a
      // change that quietly drops a screen's worth of tests trips the floor
      // while ordinary movement does not. Left at 80/80/75/70 they were so
      // far below the real figures (92/94/89/88) that a tenth of the suite
      // could have gone missing without anything failing.
      thresholds: { lines: 89, statements: 87, functions: 84, branches: 83 },
    },
  },
});
