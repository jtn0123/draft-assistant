import type { TauriCapabilities } from "@wdio/tauri-service";
import { fileURLToPath } from "node:url";
import { dirname, resolve } from "node:path";

const here = dirname(fileURLToPath(import.meta.url));

// What `npm run test:e2e` builds, via `tauri build --features wdio
// --no-bundle` rather than `cargo build --release`. Two details, both of
// which cost a run to learn:
//
//   - **The Tauri CLI, not cargo.** `generate_context!` embeds `dist/` only
//     when the `tauri/custom-protocol` feature is on, and the CLI is what
//     turns it on. Build the same code with plain cargo -- release or debug
//     -- and the webview is pointed at the `devUrl`, http://localhost:1420
//     instead. The window then comes up blank unless a Vite dev server
//     happens to be running, and goes green only by accident if one is,
//     against the dev server rather than the built bundle. Getting this
//     wrong does not break the test, which is what makes it worth a comment:
//     it quietly tests the wrong thing.
//   - **Its own target directory.** `--features wdio` produces a binary with
//     a WebDriver server in it, and that must never be mistaken for -- or
//     overwrite -- what `npm run tauri build` leaves in `target/release`.
const APP_BINARY = resolve(here, "../src-tauri/target/wdio/release/draft-assistant");

// A separate binding, not an inline literal: `tauri:options` is a
// vendor-prefixed capability key that WebdriverIO's own `Capabilities` type
// does not know about, so an object literal in place trips excess-property
// checking. `TauriCapabilities` is the service's own type for exactly this.
const capabilities: TauriCapabilities[] = [
  {
    browserName: "tauri",
    "tauri:options": { application: APP_BINARY },
  },
];

export const config: WebdriverIO.Config = {
  runner: "local",
  tsConfigPath: resolve(here, "tsconfig.json"),
  specs: [resolve(here, "specs/**/*.e2e.ts")],

  // One window, one worker. This drives a real desktop app against the real
  // Sleeper API; running two at once would just fight over the network and
  // the app's data directory.
  maxInstances: 1,
  capabilities,

  // `embedded` is the default and the only provider that works on macOS:
  // the WebDriver server is inside the app (tauri-plugin-wdio-webdriver)
  // rather than an external `tauri-driver`, which is Windows/Linux only.
  services: [["@wdio/tauri-service", { driverProvider: "embedded" }]],

  framework: "mocha",
  reporters: ["spec"],
  // Kept at "warn" rather than turned down. The run is noisy: every few
  // seconds the service logs `Tauri core.invoke not available` because it
  // keeps probing for `browser.tauri.execute()` -- its JS-injection and
  // mocking API. That API needs `withGlobalTauri: true` in tauri.conf.json
  // and an `@wdio/tauri-plugin` import inside the app's own frontend bundle:
  // a global `__TAURI__` on `window` and test code in the shipped
  // `index-*.js` respectively. Neither is worth adding to production for a
  // smoke test that drives the app through plain WebDriver, so the probe
  // fails and says so. Turning the log level down would hide real service
  // warnings along with it; the noise is the honest cost of not touching the
  // shipped bundle.
  logLevel: "warn",

  // Cold start of a debug build plus a real Sleeper fetch. Generous, because
  // a slow network here should read as slow, not as a failure.
  connectionRetryTimeout: 120_000,
  waitforTimeout: 60_000,
  mochaOpts: { ui: "bdd", timeout: 180_000 },
};
