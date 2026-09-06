// The version lives in three files that nothing keeps in step: package.json,
// src-tauri/Cargo.toml and src-tauri/tauri.conf.json. Tauri stamps the .dmg
// name and the About box from tauri.conf.json, so a release tagged v0.3.0
// against a stale conf ships an installer that calls itself 0.2.0 and
// overwrites the previous build with the same name. This fails the release
// before anything is built.
//
// With a tag argument it also checks the tag matches; with no argument it only
// checks the three files agree, which is what `npm run verify:mid` wants.

import { readFile } from "node:fs/promises";

/**
 * Pure comparison. Returns a human-readable complaint, or null when the
 * versions line up.
 *
 * @param {string | null | undefined} tag git tag such as `v0.3.0`, or nothing
 * @param {{ pkg: string, cargo: string, tauri: string }} versions
 * @returns {string | null}
 */
export function versionMismatch(tag, { pkg, cargo, tauri }) {
  const found = [
    ["package.json", pkg],
    ["src-tauri/Cargo.toml", cargo],
    ["src-tauri/tauri.conf.json", tauri],
  ];

  const missing = found.filter(([, value]) => !value).map(([file]) => file);
  if (missing.length > 0) {
    return `No version found in ${missing.join(", ")}.`;
  }

  const distinct = [...new Set(found.map(([, value]) => value))];
  if (distinct.length > 1) {
    const detail = found.map(([file, value]) => `  ${file}: ${value}`).join("\n");
    return `The three version numbers disagree:\n${detail}`;
  }

  if (tag === null || tag === undefined || tag === "") return null;

  // Tags are written `v0.3.0`; the files hold a bare `0.3.0`.
  const wanted = tag.startsWith("v") ? tag.slice(1) : tag;
  if (wanted !== distinct[0]) {
    return `Tag ${tag} does not match the version in the repo (${distinct[0]}). Bump the three files, commit, then retag.`;
  }
  return null;
}

/**
 * The `version` of the `[package]` table, not merely the first `version =` in
 * the file: `[dependencies]` entries have one too, and matching those would
 * compare the release against some crate's number.
 *
 * @param {string} toml
 * @returns {string | null}
 */
export function cargoPackageVersion(toml) {
  const start = toml.indexOf("[package]");
  if (start === -1) return null;
  const rest = toml.slice(start + "[package]".length);
  // Stop at the next table header, so only the [package] table is searched.
  const end = rest.search(/^\s*\[/m);
  const section = end === -1 ? rest : rest.slice(0, end);
  const match = section.match(/^\s*version\s*=\s*"([^"]+)"/m);
  return match === null ? null : match[1];
}

/** @param {string} json */
function jsonVersion(json) {
  const value = JSON.parse(json).version;
  return typeof value === "string" ? value : null;
}

async function main() {
  const root = new URL("../", import.meta.url);
  const read = (path) => readFile(new URL(path, root), "utf8");

  const [pkg, cargo, tauri] = await Promise.all([
    read("package.json"),
    read("src-tauri/Cargo.toml"),
    read("src-tauri/tauri.conf.json"),
  ]);

  const tag = process.argv[2];
  const problem = versionMismatch(tag, {
    pkg: jsonVersion(pkg),
    cargo: cargoPackageVersion(cargo),
    tauri: jsonVersion(tauri),
  });

  if (problem !== null) {
    console.error(problem);
    process.exit(1);
  }
  console.log(tag ? `Version ${tag} matches all three files.` : "Version numbers agree.");
}

// Only the CLI touches the filesystem, so the test can import the pure parts.
if (process.argv[1] && import.meta.url === new URL(`file://${process.argv[1]}`).href) {
  await main();
}
