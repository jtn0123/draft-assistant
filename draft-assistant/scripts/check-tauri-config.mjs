// Pins the security-relevant half of src-tauri/tauri.conf.json.
//
// Three things in that file are one careless edit away from breaking the app
// quietly, and none of them had a test:
//
//   - the CSP. `connect-src` is what lets a follower window reach the host it
//     is following (http/https and the two websocket schemes) and what lets
//     the webview reach the IPC at all. Narrowing it back to `'self'` breaks
//     the companion and the follower with a console error nobody sees, and
//     widening `default-src` or `script-src` quietly undoes the point of
//     having a policy.
//   - `createUpdaterArtifacts`. Off, `tauri build` still produces a .dmg, the
//     release publishes, and no installed copy can ever update again.
//   - the updater feed: the public key and the endpoint. A build whose pubkey
//     does not match the key the release is signed with refuses every update
//     it is offered, and an endpoint pointing at another repo polls a feed
//     that will never mention this app.
//
// Run by `npm run test:scripts` through check-tauri-config.test.mjs; the
// checks are pure so the test can feed them broken configs too.

import { readFile } from "node:fs/promises";

import { REPO } from "./updater-manifest.mjs";

/** Directives whose value is pinned exactly. */
const REQUIRED_SOURCES = {
  "default-src": ["'self'"],
  // The IPC, plus whatever host the follower has been pointed at.
  "connect-src": ["'self'", "ipc:", "http://ipc.localhost", "http:", "https:", "ws:", "wss:"],
  // Sleeper's headshots, and the asset protocol the app serves its own from.
  "img-src": ["'self'", "data:", "asset:", "http://asset.localhost", "https://sleepercdn.com"],
};

/** Directives that must not appear at all, however tempting. */
const FORBIDDEN_SOURCES = { "default-src": ["*", "'unsafe-eval'", "'unsafe-inline'"] };

/** The feed every installed copy polls. */
export const UPDATER_ENDPOINT = `https://github.com/${REPO}/releases/latest/download/latest.json`;

/**
 * The CSP as a map from directive to its sources. A directive spelled twice
 * is read the way a browser reads it: the first one counts and the rest are
 * ignored, so a second, wider copy cannot be used to talk past the first.
 *
 * @param {string} csp
 */
export function directives(csp) {
  const found = new Map();
  for (const clause of csp.split(";")) {
    const [name, ...sources] = clause.trim().split(/\s+/);
    if (name && !found.has(name)) found.set(name, sources);
  }
  return found;
}

/**
 * Everything wrong with the CSP, as sentences.
 *
 * @param {unknown} csp
 * @returns {string[]}
 */
export function cspProblems(csp) {
  if (typeof csp !== "string" || csp.trim() === "") {
    return ["app.security.csp is missing: the webview would run with no policy at all"];
  }
  const found = directives(csp);
  const problems = [];
  for (const [name, wanted] of Object.entries(REQUIRED_SOURCES)) {
    const sources = found.get(name);
    if (sources === undefined) {
      problems.push(`the CSP has no ${name} directive`);
      continue;
    }
    for (const source of wanted) {
      if (!sources.includes(source)) problems.push(`the CSP's ${name} no longer allows ${source}`);
    }
  }
  for (const [name, banned] of Object.entries(FORBIDDEN_SOURCES)) {
    for (const source of banned) {
      if ((found.get(name) ?? []).includes(source)) {
        problems.push(`the CSP's ${name} allows ${source}, which defeats the policy`);
      }
    }
  }
  return problems;
}

/**
 * Everything wrong with the bundle and updater settings, as sentences.
 *
 * @param {Record<string, any>} config the parsed tauri.conf.json
 * @returns {string[]}
 */
export function updaterProblems(config) {
  const problems = [];
  const bundle = config.bundle ?? {};
  if (bundle.createUpdaterArtifacts !== true) {
    problems.push(
      "bundle.createUpdaterArtifacts is off, so a release would ship a .dmg no installed copy can update from",
    );
  }
  if (!bundle.macOS?.signingIdentity) {
    problems.push("bundle.macOS.signingIdentity is unset, so the .app is not even ad-hoc signed");
  }
  const updater = config.plugins?.updater ?? {};
  const pubkey = typeof updater.pubkey === "string" ? updater.pubkey : "";
  const decoded = pubkey === "" ? "" : Buffer.from(pubkey, "base64").toString("utf8");
  if (!decoded.includes("minisign public key")) {
    problems.push("plugins.updater.pubkey is not a minisign public key, so no update can verify");
  }
  const endpoints = Array.isArray(updater.endpoints) ? updater.endpoints : [];
  if (endpoints.length !== 1 || endpoints[0] !== UPDATER_ENDPOINT) {
    problems.push(
      `plugins.updater.endpoints must be exactly [${UPDATER_ENDPOINT}], the feed the release attaches latest.json to`,
    );
  }
  return problems;
}

/**
 * Both halves, for one parsed config.
 *
 * @param {Record<string, any>} config
 * @returns {string[]}
 */
export function configProblems(config) {
  return [...cspProblems(config.app?.security?.csp), ...updaterProblems(config)];
}

async function main() {
  const path = new URL("../src-tauri/tauri.conf.json", import.meta.url);
  const problems = configProblems(JSON.parse(await readFile(path, "utf8")));
  if (problems.length > 0) {
    console.error(`src-tauri/tauri.conf.json:\n${problems.map((p) => `  ${p}`).join("\n")}`);
    process.exit(1);
  }
  console.log("tauri.conf.json: CSP, updater artifacts and the update feed are as pinned");
}

// Only the CLI reads the file, so the test can import the pure parts.
if (process.argv[1] && import.meta.url === new URL(`file://${process.argv[1]}`).href) {
  await main();
}
