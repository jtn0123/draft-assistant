import react from "@vitejs/plugin-react";
import { defineConfig } from "vitest/config";

export default defineConfig({
  plugins: [react()],
  test: {
    environment: "jsdom",
    globals: true,
    setupFiles: "./src/test/setup.ts",
    css: true,
    coverage: {
      provider: "v8",
      // The floor, not the goal — raise these as coverage climbs. Enforced by
      // `npm run test:coverage`, which `npm run verify` (and therefore CI) runs.
      thresholds: { lines: 80, statements: 80, functions: 75, branches: 70 },
    },
  },
});
