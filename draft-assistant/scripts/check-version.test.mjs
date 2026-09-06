// Run by `npm run test:scripts` (node --test). The vitest suite only collects
// src/, so guard scripts get their tests here.

import assert from "node:assert/strict";
import { test } from "node:test";

import { cargoPackageVersion, versionMismatch } from "./check-version.mjs";

const agreed = { pkg: "0.2.0", cargo: "0.2.0", tauri: "0.2.0" };

test("no argument passes when the three files agree", () => {
  assert.equal(versionMismatch(undefined, agreed), null);
});

test("a matching tag passes with or without the leading v", () => {
  assert.equal(versionMismatch("v0.2.0", agreed), null);
  assert.equal(versionMismatch("0.2.0", agreed), null);
});

test("a tag ahead of the repo is rejected, naming both numbers", () => {
  const problem = versionMismatch("v9.9.9", agreed);
  assert.match(problem, /v9\.9\.9/);
  assert.match(problem, /0\.2\.0/);
});

test("a prerelease tag still has to match exactly", () => {
  assert.equal(
    versionMismatch("v0.3.0-rc1", { pkg: "0.3.0-rc1", cargo: "0.3.0-rc1", tauri: "0.3.0-rc1" }),
    null,
  );
  assert.notEqual(
    versionMismatch("v0.3.0-rc1", { pkg: "0.3.0", cargo: "0.3.0", tauri: "0.3.0" }),
    null,
  );
});

test("a half-bumped repo fails even with no tag, listing every file", () => {
  const problem = versionMismatch(undefined, { pkg: "0.3.0", cargo: "0.2.0", tauri: "0.3.0" });
  assert.match(problem, /Cargo\.toml/);
  assert.match(problem, /tauri\.conf\.json/);
  assert.match(problem, /package\.json/);
});

test("a missing version is reported rather than treated as agreement", () => {
  const problem = versionMismatch("v0.2.0", { pkg: "0.2.0", cargo: null, tauri: "0.2.0" });
  assert.match(problem, /No version found in src-tauri\/Cargo\.toml/);
});

test("the Cargo version comes from [package], not from a dependency", () => {
  const toml = [
    "[package]",
    'name = "draft-assistant"',
    'version = "0.2.0"',
    "",
    "[dependencies]",
    'serde = { version = "1.0.219" }',
  ].join("\n");
  assert.equal(cargoPackageVersion(toml), "0.2.0");
});

test("a dependency table ahead of [package] does not win", () => {
  const toml = [
    "[dependencies]",
    'serde = { version = "1.0.219" }',
    "",
    "[package]",
    'version = "0.4.1"',
  ].join("\n");
  assert.equal(cargoPackageVersion(toml), "0.4.1");
});

test("a Cargo.toml with no [package] table reports no version", () => {
  assert.equal(cargoPackageVersion('[workspace]\nmembers = ["a"]\n'), null);
});
