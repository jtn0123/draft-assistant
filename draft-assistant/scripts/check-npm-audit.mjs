// `npm audit --audit-level=high` on its own has no way to accept a single
// advisory: the only lever is the level, and turning it down to get past
// one unfixable dev dependency hides every future high in both trees. This
// runs the audit as JSON and fails on any high or critical advisory that is
// not named in scripts/npm-audit-allowlist.json, with its reason beside it.
//
// Usage: node scripts/check-npm-audit.mjs <dir> [...more dirs]
// Each dir needs a package-lock.json; node_modules is not required.

import { spawnSync } from "node:child_process";
import { readFile } from "node:fs/promises";

const GATED = new Set(["high", "critical"]);

/**
 * Advisory ids (GHSA-...) at the gated severities, one per distinct advisory.
 * npm lists a vulnerability per package; the advisory objects sit in `via`,
 * and a string in `via` only names another package already in the list.
 *
 * @param {{ vulnerabilities?: Record<string, { via: Array<string | { url?: string, severity: string, title?: string, name?: string }> }> }} audit
 * @returns {Map<string, string>} id -> "package: title"
 */
export function gatedAdvisories(audit) {
  const found = new Map();
  for (const entry of Object.values(audit.vulnerabilities ?? {})) {
    for (const via of entry.via) {
      if (typeof via !== "object" || !GATED.has(via.severity)) continue;
      const id = advisoryId(via.url);
      if (!found.has(id)) found.set(id, `${via.name ?? "?"}: ${via.title ?? via.url ?? id}`);
    }
  }
  return found;
}

/** @param {string | undefined} url */
function advisoryId(url) {
  if (!url) return "unknown";
  const parts = url.split("/");
  return parts[parts.length - 1] || url;
}

/**
 * The verdict for one tree.
 *
 * @param {Map<string, string>} found from gatedAdvisories
 * @param {Record<string, string>} allowlist id -> reason
 * @returns {{ blocking: string[], stale: string[] }}
 */
export function verdict(found, allowlist) {
  const blocking = [...found]
    .filter(([id]) => !(id in allowlist))
    .map(([id, what]) => `${id} (${what})`);
  const stale = Object.keys(allowlist).filter((id) => !found.has(id));
  return { blocking, stale };
}

/** @param {string} dir */
function runAudit(dir) {
  // npm exits non-zero whenever it finds anything, so the exit code says
  // nothing the JSON does not; only a missing body is a failure to run.
  const result = spawnSync("npm", ["audit", "--json", "--audit-level=high"], {
    cwd: dir,
    encoding: "utf8",
    shell: process.platform === "win32",
  });
  if (!result.stdout) {
    throw new Error(`npm audit produced no output in ${dir}: ${result.stderr || result.error}`);
  }
  return JSON.parse(result.stdout);
}

async function main() {
  const dirs = process.argv.slice(2);
  if (dirs.length === 0) {
    console.error("usage: node scripts/check-npm-audit.mjs <dir> [...more dirs]");
    process.exit(2);
  }
  const allowFile = new URL("./npm-audit-allowlist.json", import.meta.url);
  const { advisories } = JSON.parse(await readFile(allowFile, "utf8"));

  // Stale entries are judged across every tree given, since one allowlist
  // serves both: an advisory fixed in e2e/ should not read as stale merely
  // because the app tree never had it.
  const seen = new Set();
  let failed = false;
  for (const dir of dirs) {
    const found = gatedAdvisories(runAudit(dir));
    for (const id of found.keys()) seen.add(id);
    const { blocking } = verdict(found, advisories);
    if (blocking.length > 0) {
      failed = true;
      console.error(`${dir}: high or critical advisories not in the allowlist:`);
      for (const line of blocking) console.error(`  ${line}`);
    } else {
      console.log(`${dir}: no high or critical advisories outside the allowlist.`);
    }
  }
  for (const id of Object.keys(advisories)) {
    if (!seen.has(id)) console.warn(`allowlist entry ${id} no longer reported; remove it.`);
  }
  if (failed) process.exit(1);
}

// Only the CLI runs npm, so the test can import the pure parts.
if (process.argv[1] && import.meta.url === new URL(`file://${process.argv[1]}`).href) {
  await main();
}
