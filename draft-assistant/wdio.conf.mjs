// Desktop end-to-end: the real Tauri binary, the real Rust backend, driven
// through WebdriverIO's embedded driver (the only WebDriver that reaches a
// macOS WKWebView). Everything else in `e2e/` tests the same UI in a browser
// against a recorded dump; this is the app itself.
//
//   bun run test:desktop        (builds the binary first — see package.json)
//
// The app under test runs on a copy of the real cache in a scratch directory,
// so a test can never write to the league you are playing.
import { cpSync, mkdirSync, rmSync, existsSync } from "node:fs";
import { homedir, tmpdir } from "node:os";
import { join, resolve } from "node:path";

const binary = resolve("src-tauri/target/release/draft-assistant");
const dataDir = join(tmpdir(), "draft-assistant-wdio");
const realData = join(homedir(), "Library/Application Support/com.justin.draft-assistant");

export const config = {
  runner: "local",
  specs: ["./e2e-desktop/**/*.spec.mjs"],
  maxInstances: 1,
  framework: "mocha",
  reporters: ["spec"],
  logLevel: "warn",
  mochaOpts: { ui: "bdd", timeout: 120_000 },
  services: [
    [
      "@wdio/tauri-service",
      {
        appBinaryPath: binary,
        driverProvider: "embedded",
        captureBackendLogs: true,
        captureFrontendLogs: true,
        startTimeout: 60_000,
      },
    ],
  ],
  capabilities: [{ browserName: "tauri", "tauri:options": { application: binary } }],
  onPrepare() {
    if (!existsSync(binary)) {
      throw new Error(`no desktop binary at ${binary} — run: bun run build:desktop`);
    }
    rmSync(dataDir, { recursive: true, force: true });
    mkdirSync(dataDir, { recursive: true });
    // A copy of the caches, so the app boots into the real league without a
    // cold fetch and without touching the original.
    if (existsSync(realData)) {
      for (const name of [
        "config.json",
        "players.json",
        "projections_2026.json",
        "weekly_2026.json",
        "draft-state.json",
      ]) {
        const from = join(realData, name);
        if (existsSync(from)) cpSync(from, join(dataDir, name));
      }
    }
    process.env.DRAFT_ASSISTANT_DATA_DIR = dataDir;
  },
};
