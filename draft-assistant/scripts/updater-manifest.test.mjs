// Run by `npm run test:scripts` (node --test). The vitest suite only collects
// src/, so guard scripts get their tests here.

import assert from "node:assert/strict";
import { test } from "node:test";

import {
  assetName,
  manifest,
  missingAssets,
  pickArchive,
  platformFor,
  platformKey,
} from "./updater-manifest.mjs";

test("the published asset name has no spaces and carries the version", () => {
  const name = assetName("v0.3.0", "aarch64");
  assert.equal(name, "Draft-Assistant-0.3.0-aarch64.app.tar.gz");
  assert.doesNotMatch(name, /\s/);
});

test("latest.json points at the asset under the tag, with a bare version", () => {
  const body = manifest({
    tag: "v0.3.0",
    arch: "aarch64",
    signature: "untrusted comment: sig\nABC\n",
    pubDate: "2026-09-05T00:00:00.000Z",
  });
  assert.equal(body.version, "0.3.0");
  assert.equal(body.pub_date, "2026-09-05T00:00:00.000Z");
  const mac = body.platforms["darwin-aarch64"];
  assert.equal(
    mac.url,
    "https://github.com/jtn0123/draft-assistant/releases/download/v0.3.0/Draft-Assistant-0.3.0-aarch64.app.tar.gz",
  );
  assert.equal(mac.signature, "untrusted comment: sig\nABC");
});

test("an archive without its .sig is refused rather than published unsigned", () => {
  assert.deepEqual(pickArchive(["Draft Assistant.app.tar.gz", "Draft Assistant.app.tar.gz.sig"]), {
    archive: "Draft Assistant.app.tar.gz",
  });
  assert.match(pickArchive(["Draft Assistant.app.tar.gz"]).error, /no \.sig/);
  assert.match(pickArchive(["Draft Assistant.app"]).error, /found: Draft Assistant\.app/);
});

test("the platform key is named by the caller, not taken from the runner", () => {
  // The failure this prevents: the key was `darwin-${process.arch}`, right on
  // today's runner and silently wrong the day the image changes. Every
  // install would then poll a manifest with no key for its own Mac and be
  // told there is no build for it.
  assert.deepEqual(platformKey("aarch64", "arm64"), { key: "darwin-aarch64", arch: "aarch64" });
  assert.deepEqual(platformKey("x86_64", "x64"), { key: "darwin-x86_64", arch: "x86_64" });
});

test("a manifest for an arch the runner did not build is refused", () => {
  const wrong = platformKey("aarch64", "x64");
  assert.match(wrong.error, /built on x86_64/);
  assert.match(wrong.error, /no build for this Mac/);
  assert.match(platformKey(undefined, "arm64").error, /name the arch/);
  assert.match(platformKey("riscv", "arm64").error, /unknown arch riscv/);
  assert.throws(() => platformFor("riscv"), /unknown arch riscv/);
  assert.equal(platformFor("aarch64"), "darwin-aarch64");
});

test("a release missing any of the three updater assets is incomplete", () => {
  // The failure this prevents: the .dmg was published and became `latest`
  // before latest.json was attached, so an install polling
  // releases/latest/download/latest.json got a 404 and every "Check for
  // updates" answered "No release feed yet" from then on.
  const asset = assetName("v0.3.0", "aarch64");
  const complete = [asset, `${asset}.sig`, "latest.json"];
  assert.deepEqual(missingAssets(complete, "v0.3.0", "aarch64"), []);
  assert.deepEqual(missingAssets([asset, "latest.json"], "v0.3.0", "aarch64"), [`${asset}.sig`]);
  assert.deepEqual(missingAssets(complete.slice(0, 2), "v0.3.0", "aarch64"), ["latest.json"]);
  assert.deepEqual(missingAssets([], "v0.3.0", "aarch64"), complete);
  // A manifest from a different tag does not count as this release's.
  assert.deepEqual(missingAssets(complete, "v0.4.0", "aarch64").length, 2);
});
