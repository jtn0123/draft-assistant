// The version the browser preview shows sat at 0.2.0 through two releases,
// because it was a string somebody had to remember. Now it is read from
// package.json at build time, and this holds it there.

import { readFileSync } from "node:fs";
import { describe, expect, it } from "vitest";
import { PREVIEW_VERSION } from "./appVersion";

function versionIn(path: string): unknown {
  return (JSON.parse(readFileSync(new URL(path, import.meta.url), "utf8")) as { version?: unknown })
    .version;
}

describe("the preview version", () => {
  it("is the version in package.json, not a string kept by hand", () => {
    expect(PREVIEW_VERSION).toBe(versionIn("../package.json"));
    expect(PREVIEW_VERSION).toMatch(/^\d+\.\d+\.\d+/);
  });

  it("is what the shipped app calls itself too", () => {
    // scripts/check-version.mjs holds the three version files together at
    // release time; this is the same promise, made where the preview reads.
    expect(PREVIEW_VERSION).toBe(versionIn("../src-tauri/tauri.conf.json"));
  });
});
