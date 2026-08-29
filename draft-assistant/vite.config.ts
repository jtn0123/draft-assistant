import { defineConfig } from "vite";
import react from "@vitejs/plugin-react";

// @ts-expect-error process is a nodejs global
const host = process.env.TAURI_DEV_HOST;

// https://vite.dev/config/
export default defineConfig(async () => ({
  plugins: [react()],

  // Vite options tailored for Tauri development and only applied in `tauri dev` or `tauri build`
  //
  // 1. prevent Vite from obscuring rust errors
  clearScreen: false,
  // 2. tauri expects a fixed port, fail if that port is not available
  server: {
    port: 1420,
    strictPort: true,
    host: host || false,
    hmr: host
      ? {
          protocol: "ws",
          host,
          port: 1421,
        }
      : undefined,
    watch: {
      // 3. tell Vite to ignore watching `src-tauri`, and everything the test
      //    suite and the dogfood scripts write into the worktree — otherwise a
      //    coverage run or a Playwright report reloads the live `tauri dev`
      //    window (observed 2026-08-28 12:37) and wipes the chat panel.
      ignored: [
        "**/src-tauri/**",
        "**/coverage/**",
        "**/playwright-report/**",
        "**/test-results/**",
        "**/dogfood-output/**",
        "**/public/ai-*.json",
      ],
    },
  },
}));
