// Run by `npm run test:scripts` (node --test). The vitest suite only collects
// src/, so guard scripts get their tests here.
//
// The dashes in this file are the subject under test, which is why
// `check-dashes.mjs` skips `scripts/` entirely.

import assert from "node:assert/strict";
import { test } from "node:test";

import { findDashes, stripComments } from "./check-dashes.mjs";

test("a dash in a string literal is reported, with its line number", () => {
  const source = ['const a = "fine";', 'const b = "half — gone";'].join("\n");
  assert.deepEqual(findDashes(source, "js"), [2]);
});

test("an en-dash counts too, and both spellings of an escape", () => {
  assert.deepEqual(findDashes('t("a – b")', "js"), [1]);
  assert.deepEqual(findDashes('t("a \\u2014 b")', "js"), [1]);
  assert.deepEqual(findDashes('t("a \\u{2014} b")', "rust"), [1]);
});

test("a dash a reader never sees is left alone", () => {
  assert.deepEqual(findDashes("// written for people — not shipped", "js"), []);
  assert.deepEqual(findDashes("/// a doc comment — also fine", "rust"), []);
  assert.deepEqual(findDashes("<!-- a page comment — fine -->", "html"), []);
});

test("a block comment hides a dash across every line it spans", () => {
  const source = ["/* opening", " * a dash — in the middle", " */", 'say("clean");'].join("\n");
  assert.deepEqual(findDashes(source, "js"), []);
});

test("code after a block comment closes is read again", () => {
  assert.deepEqual(findDashes('/* quiet */ say("loud — here");', "js"), [1]);
});

test("a URL inside a string is not mistaken for the start of a comment", () => {
  const source = 'const url = "https://example.com/a"; // — fine\nconst t = "x — y";';
  assert.deepEqual(findDashes(source, "js"), [2]);
});

test("a Rust lifetime does not open a string and swallow the rest of the file", () => {
  const source = ["fn f<'a>(s: &'a str) {}", 'const T: &str = "a — b";'].join("\n");
  assert.deepEqual(findDashes(source, "rust"), [2]);
});

test("a dash in a Rust char literal still counts: it reaches the user the same way", () => {
  assert.deepEqual(findDashes("let c = '—';", "rust"), [1]);
});

test("stripComments hands back the code and whether a block is still open", () => {
  assert.deepEqual(stripComments('say("hi"); // trailing', { inBlock: false }, "js"), {
    code: 'say("hi"); ',
    inBlock: false,
  });
  assert.equal(stripComments("/* opened", { inBlock: false }, "js").inBlock, true);
  assert.equal(stripComments("still inside", { inBlock: true }, "js").code, "");
});
