// Run by `npm run test:scripts` (node --test). The vitest suite only collects
// src/, so guard scripts get their tests here.

import assert from "node:assert/strict";
import { test } from "node:test";
import { sep } from "node:path";

import { MAX_LINES, isExcluded, lineCount, oversized } from "./check-loc.mjs";

/** A file of `lines` lines, as the bytes the check reads. */
const source = (lines) => Buffer.from(Array.from({ length: lines }, (_, i) => `l${i}`).join("\n"));

test("a file over the cap is named with its count", () => {
  assert.deepEqual(oversized([["src/App.tsx", source(MAX_LINES + 1)]]), [
    ["src/App.tsx", MAX_LINES + 1],
  ]);
});

test("a file at exactly the cap passes, and so does one under it", () => {
  assert.deepEqual(oversized([["src/App.tsx", source(MAX_LINES)]]), []);
  assert.deepEqual(oversized([["src/App.tsx", source(1)]]), []);
  assert.deepEqual(oversized([["src/empty.ts", Buffer.alloc(0)]]), []);
});

test("only the oversized files are reported, in the order they were read", () => {
  const over = oversized([
    ["a.ts", source(10)],
    ["b.ts", source(600)],
    ["c.ts", source(MAX_LINES)],
    ["d.ts", source(501)],
  ]);
  assert.deepEqual(over, [
    ["b.ts", 600],
    ["d.ts", 501],
  ]);
});

test("a trailing newline costs a line, as it does in an editor", () => {
  assert.equal(lineCount(Buffer.from("a\nb")), 2);
  assert.equal(lineCount(Buffer.from("a\nb\n")), 3);
  assert.equal(lineCount(Buffer.from("a\r\nb")), 2);
});

test("a binary is not counted at all, however long it is", () => {
  const binary = Buffer.concat([Buffer.from([0]), source(900)]);
  assert.equal(lineCount(binary), null);
  assert.deepEqual(oversized([["icon.png", binary]]), []);
});

test("generated and vendored trees are excluded by name", () => {
  for (const directory of ["node_modules", "target", "dist", "coverage", "gen", ".git"]) {
    assert.equal(isExcluded(directory, `draft-assistant${sep}${directory}`), true, directory);
  }
  assert.equal(isExcluded("package-lock.json", "package-lock.json"), true);
  assert.equal(isExcluded("Cargo.lock", `draft-assistant${sep}src-tauri${sep}Cargo.lock`), true);
});

test("the two big fixtures are excluded by their path, not by their name", () => {
  const fixture = `draft-assistant${sep}public${sep}dev-fixture.json`;
  assert.equal(isExcluded("dev-fixture.json", fixture), true);
  // The same name somewhere else is still source, and still capped.
  assert.equal(isExcluded("dev-fixture.json", `e2e${sep}dev-fixture.json`), false);
});

test("ordinary source is not excluded", () => {
  assert.equal(isExcluded("App.tsx", `draft-assistant${sep}src${sep}App.tsx`), false);
  assert.equal(isExcluded("src", `draft-assistant${sep}src`), false);
});
