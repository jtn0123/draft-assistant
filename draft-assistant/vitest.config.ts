import react from "@vitejs/plugin-react";
import { defineConfig } from "vitest/config";

export default defineConfig({
  plugins: [react()],
  test: {
    environment: "jsdom",
    // jsdom renders the board for real; a loaded CI runner is several times
    // slower than a laptop, and 5 s left no headroom.
    testTimeout: 20_000,
    globals: true,
    setupFiles: "./src/test/setup.ts",
    css: true,
    // e2e/ belongs to Playwright, which has its own runner and expect().
    include: ["src/**/*.{test,spec}.{ts,tsx}"],
    exclude: ["e2e/**", "node_modules/**"],
  },
});
