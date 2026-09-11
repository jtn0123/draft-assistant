// Preserve real filenames and, in the dedicated coverage run, share counters
// across each test file's VM contexts. Nothing changes in the shipped scripts.
import { readFileSync, mkdirSync, writeFileSync } from "node:fs";
import { resolve } from "node:path";
import { randomUUID } from "node:crypto";
import { createContext, isContext, runInContext, type Context } from "node:vm";
import { createInstrumenter } from "istanbul-lib-instrument";
import { afterAll } from "vitest";

const coverage: Record<string, unknown> = {};
const instrumenter = createInstrumenter({ coverageVariable: "__companion_coverage__" });
const instrumented = new Map<string, string>();
const output = process.env.COMPANION_COVERAGE_DIR;
if (output) {
  afterAll(() => {
    mkdirSync(output, { recursive: true });
    writeFileSync(resolve(output, `${process.pid}-${randomUUID()}.json`), JSON.stringify(coverage));
  });
}

export function runCompanionScript(file: string, sandbox: Context): void {
  const filename = resolve("src-tauri/companion-static", file);
  let source = readFileSync(filename, "utf8");
  if (output) {
    source = instrumented.get(filename) ?? instrumenter.instrumentSync(source, filename);
    instrumented.set(filename, source);
    sandbox.__companion_coverage__ = coverage;
  }
  runInContext(source, isContext(sandbox) ? sandbox : createContext(sandbox), { filename });
}
