// Run by `npm run test:scripts` (node --test). The vitest suite only collects
// src/, so guard scripts get their tests here.

import assert from "node:assert/strict";
import { test } from "node:test";

import { classOwners, sharedClasses } from "./check-css.mjs";

const owners = (sheets) =>
  Object.fromEntries([...classOwners(sheets)].map(([name, where]) => [name, [...where].sort()]));

test("a class styled from two stylesheets is caught, and named with both files", () => {
  const shared = sharedClasses([
    ["bits.css", ".pill { color: red; }"],
    ["board.css", ".board-row { gap: 4px; }\n.pill { gap: 2px; }"],
  ]);
  assert.deepEqual(shared, [["pill", ["bits.css", "board.css"]]]);
});

test("a class styled from a single sheet is not", () => {
  const sheets = [
    ["bits.css", ".pill { color: red; }\n.pill:hover { color: blue; }\n.pill .dot { top: 0; }"],
    ["board.css", ".board-row { gap: 4px; }"],
  ];
  assert.deepEqual(sharedClasses(sheets), []);
});

test("the first rule inside an at-rule block is read like any other", () => {
  const sheets = [
    ["bits.css", "@media (max-width: 600px) {\n  .pill { gap: 1px; }\n}"],
    ["board.css", ".pill { gap: 2px; }"],
  ];
  assert.deepEqual(sharedClasses(sheets), [["pill", ["bits.css", "board.css"]]]);
  // The at-rule itself carries no class, so it owns nothing of its own.
  assert.deepEqual(Object.keys(owners([sheets[0]])), ["pill"]);
});

test("every rule in a selector list is charged to its own first class", () => {
  assert.deepEqual(
    owners([["bits.css", '.a, .b { top: 0; }\n[data-theme="dark"] .c { top: 1px; }']]),
    {
      a: ["bits.css"],
      b: ["bits.css"],
      c: ["bits.css"],
    },
  );
});

test("a class named only inside a comment owns nothing", () => {
  assert.deepEqual(owners([["bits.css", "/* .ghost { color: red; } */\n.real { color: red; }"]]), {
    real: ["bits.css"],
  });
});

test("two sheets that share nothing are reported in class order", () => {
  const shared = sharedClasses([
    ["a.css", ".zeta { top: 0; }\n.alpha { top: 0; }"],
    ["b.css", ".alpha { top: 1px; }\n.zeta { top: 1px; }"],
  ]);
  assert.deepEqual(
    shared.map(([name]) => name),
    ["alpha", "zeta"],
  );
});
