// Run by `npm run test:scripts` (node --test). The vitest suite only collects
// src/, so guard scripts get their tests here.

import assert from "node:assert/strict";
import { readFile } from "node:fs/promises";
import { test } from "node:test";

import {
  configProblems,
  cspProblems,
  directives,
  updaterProblems,
  UPDATER_ENDPOINT,
} from "./check-tauri-config.mjs";

const shipped = JSON.parse(
  await readFile(new URL("../src-tauri/tauri.conf.json", import.meta.url), "utf8"),
);

test("the config the app actually ships with passes every pin", () => {
  assert.deepEqual(configProblems(shipped), []);
});

test("a CSP the follower could not reach its host through is caught", () => {
  // The failure this prevents: narrowing connect-src back to 'self' breaks
  // the follower window and the companion with a console error nobody sees,
  // and no test said a word about it.
  const narrowed = "default-src 'self'; connect-src 'self' ipc: http://ipc.localhost";
  const problems = cspProblems(narrowed);
  for (const source of ["http:", "https:", "ws:", "wss:"]) {
    assert.ok(
      problems.some((p) => p.includes(source)),
      `${source} going missing was not reported: ${problems.join(" | ")}`,
    );
  }
  assert.match(cspProblems(undefined)[0], /no policy at all/);
  assert.match(cspProblems("img-src 'self'")[0], /no default-src/);
});

test("a CSP that gives itself away is caught too", () => {
  const wide = `default-src 'self' * 'unsafe-eval'; ${shipped.app.security.csp}`;
  const problems = cspProblems(wide);
  assert.ok(problems.some((p) => p.includes("allows *")));
  assert.ok(problems.some((p) => p.includes("'unsafe-eval'")));
});

test("the directive scanner keeps each directive's sources together", () => {
  const found = directives("default-src 'self'; connect-src 'self' ws: wss:");
  assert.deepEqual(found.get("default-src"), ["'self'"]);
  assert.deepEqual(found.get("connect-src"), ["'self'", "ws:", "wss:"]);
  // A browser ignores the second spelling of a directive, so a wider copy
  // appended to the end must not be read as the one in force.
  assert.deepEqual(directives("default-src 'self'; default-src *").get("default-src"), ["'self'"]);
});

test("turning the updater artifacts off is caught before a release ships", () => {
  // The failure this prevents: `tauri build` still makes a .dmg with this
  // off, the release publishes, and no installed copy can ever update again.
  const off = { ...shipped, bundle: { ...shipped.bundle, createUpdaterArtifacts: false } };
  assert.match(updaterProblems(off)[0], /createUpdaterArtifacts is off/);
  const unsigned = { ...shipped, bundle: { ...shipped.bundle, macOS: {} } };
  assert.match(updaterProblems(unsigned)[0], /signingIdentity/);
});

test("the feed and the key it is verified against are both pinned", () => {
  const noKey = { ...shipped, plugins: { updater: { endpoints: [UPDATER_ENDPOINT] } } };
  assert.match(updaterProblems(noKey)[0], /minisign public key/);
  const elsewhere = {
    ...shipped,
    plugins: {
      updater: {
        ...shipped.plugins.updater,
        endpoints: ["https://github.com/someone/else/releases/latest/download/latest.json"],
      },
    },
  };
  assert.match(updaterProblems(elsewhere)[0], /endpoints must be exactly/);
  // The endpoint is the one the release attaches latest.json to, spelled the
  // same way in both places.
  assert.equal(shipped.plugins.updater.endpoints[0], UPDATER_ENDPOINT);
  assert.ok(UPDATER_ENDPOINT.endsWith("/releases/latest/download/latest.json"));
});
