// Run by `npm run test:scripts` (node --test). The vitest suite only collects
// src/, so guard scripts get their tests here.

import assert from "node:assert/strict";
import { test } from "node:test";

import { assetName, manifest, pickArchive } from "./updater-manifest.mjs";

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
