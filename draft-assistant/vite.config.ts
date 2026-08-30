import { defineConfig } from "vite";
import react from "@vitejs/plugin-react";
import { visualizer } from "rollup-plugin-visualizer";

// @ts-expect-error process is a nodejs global
const host = process.env.TAURI_DEV_HOST;

// https://vite.dev/config/
export default defineConfig(async ({ mode }) => ({
  // `npm run analyze` adds a bundle report so a heavy dependency cannot slip
  // in unnoticed; normal builds do not pay for it.
  plugins: [
    react(),
    ...(mode === "analyze" ? [visualizer({ filename: "dist/stats.html", gzipSize: true })] : []),
  ],

  build: {
    // Tauri ships a known WKWebView/WebView2, so the default browser-compat
    // target is far more conservative than this app ever needs.
    target: "es2022",
    // Kept so a stack trace from a release build is readable.
    sourcemap: true,
    chunkSizeWarningLimit: 700,
  },

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
      // 3. tell Vite to ignore watching `src-tauri`
      ignored: ["**/src-tauri/**"],
    },
  },
}));
