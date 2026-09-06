// Run by `npm run test:scripts` (node --test). The vitest suite only collects
// src/, so guard scripts get their tests here.

import assert from "node:assert/strict";
import { test } from "node:test";

import { gatedAdvisories, verdict } from "./check-npm-audit.mjs";

const extractZip = {
  url: "https://github.com/advisories/GHSA-jmr9-qjv8-65gv",
  severity: "high",
  name: "extract-zip",
  title: "unvalidated symlink path traversal",
};

/** The shape `npm audit --json` gives for one root advisory and its dependents. */
function audit(...vias) {
  return {
    vulnerabilities: {
      "extract-zip": { via: vias },
      "@wdio/utils": { via: ["extract-zip"] },
      "@wdio/cli": { via: ["@wdio/utils"] },
    },
  };
}

test("one advisory is counted once however many packages depend on it", () => {
  const found = gatedAdvisories(audit(extractZip));
  assert.deepEqual([...found.keys()], ["GHSA-jmr9-qjv8-65gv"]);
  assert.match(found.get("GHSA-jmr9-qjv8-65gv"), /extract-zip/);
});

test("moderate and low advisories are not gated", () => {
  const found = gatedAdvisories(audit({ ...extractZip, severity: "moderate" }));
  assert.equal(found.size, 0);
});

test("an allow-listed advisory passes and an unlisted high blocks", () => {
  const found = gatedAdvisories(
    audit(extractZip, {
      url: "https://github.com/advisories/GHSA-new0-new0-new0",
      severity: "critical",
      name: "something",
      title: "remote code execution",
    }),
  );
  const { blocking, stale } = verdict(found, { "GHSA-jmr9-qjv8-65gv": "known, dev tree only" });
  assert.deepEqual(blocking, ["GHSA-new0-new0-new0 (something: remote code execution)"]);
  assert.deepEqual(stale, []);
});

test("lowering nothing: a clean tree with a leftover allowlist entry is reported stale", () => {
  const { blocking, stale } = verdict(new Map(), { "GHSA-jmr9-qjv8-65gv": "fixed upstream since" });
  assert.deepEqual(blocking, []);
  assert.deepEqual(stale, ["GHSA-jmr9-qjv8-65gv"]);
});
