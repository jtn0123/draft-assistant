// Run by `npm run test:scripts` (node --test). The vitest suite only collects
// src/, so guard scripts get their tests here.

import assert from "node:assert/strict";
import { test } from "node:test";

import { gatedAdvisories, verdict, parseAuditResult } from "./check-npm-audit.mjs";

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

const cleanReport = {
  auditReportVersion: 2,
  vulnerabilities: {},
  metadata: { vulnerabilities: { total: 0 } },
};
function result(report, status = 0) {
  return { stdout: JSON.stringify(report), status };
}
test("audit operational failures never count as a clean report", () => {
  for (const failure of [
    result({ error: { code: "ENOAUDIT" } }, 1),
    result({}),
    result({ vulnerabilities: {} }),
    result({ metadata: {} }),
    { stdout: "not json", status: 1 },
    { stdout: "", error: new Error("spawn failed") },
    { ...result(cleanReport), signal: "SIGTERM" },
    result(cleanReport, 2),
  ])
    assert.throws(() => parseAuditResult(failure, "."));
});
test("a complete clean report and vulnerability exit status remain valid", () => {
  assert.deepEqual(parseAuditResult(result(cleanReport), "."), cleanReport);
  const found = { ...cleanReport, vulnerabilities: audit(extractZip).vulnerabilities };
  assert.deepEqual(parseAuditResult(result(found, 1), "e2e"), found);
});
