// Separate coverage for the shipped phone scripts, including untested files.
import { spawnSync } from "node:child_process";
import { readFileSync, readdirSync, mkdirSync, rmSync } from "node:fs";
import { resolve } from "node:path";
import { createInstrumenter } from "istanbul-lib-instrument";
import libCoverage from "istanbul-lib-coverage";
import libReport from "istanbul-lib-report";
import reports from "istanbul-reports";

const output = resolve("coverage-companion");
const temporary = resolve(output, "tmp");
rmSync(temporary, { force: true, recursive: true });
mkdirSync(temporary, { recursive: true });
const result = spawnSync(
  process.execPath,
  [
    "node_modules/vitest/vitest.mjs",
    "run",
    "companionPage",
    "companionDraftExtras",
    ...process.argv.slice(2),
  ],
  { stdio: "inherit", env: { ...process.env, COMPANION_COVERAGE_DIR: temporary } },
);
if (result.error) throw result.error;
if (result.status !== 0) process.exit(result.status ?? 1);
const coverage = libCoverage.createCoverageMap({});
const source = resolve("src-tauri/companion-static");
const instrumenter = createInstrumenter();
for (const file of readdirSync(source).filter((name) => name.endsWith(".js"))) {
  const filename = resolve(source, file);
  instrumenter.instrumentSync(readFileSync(filename, "utf8"), filename);
  coverage.addFileCoverage(instrumenter.lastFileCoverage());
}
for (const file of readdirSync(temporary)) {
  coverage.merge(JSON.parse(readFileSync(resolve(temporary, file), "utf8")));
}
const context = libReport.createContext({ dir: output, coverageMap: coverage });
for (const format of ["text", "json-summary", "html", "lcovonly"])
  reports.create(format).execute(context);
const summary = coverage.getCoverageSummary();
// Baseline 2026-09-08: 94.89 lines / 92.58 statements / 91.05 functions / 83.11 branches.
// All 15 shipped scripts count, including the untested service worker.
const floors = { lines: 90, statements: 88, functions: 86, branches: 78 };
for (const [metric, floor] of Object.entries(floors)) {
  if (summary[metric].pct < floor) {
    console.error(`Companion ${metric} coverage ${summary[metric].pct}% is below ${floor}%`);
    process.exitCode = 1;
  }
}
