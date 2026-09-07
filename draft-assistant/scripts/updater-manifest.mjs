// `tauri build` with `createUpdaterArtifacts` writes the signed archive the
// updater installs (`<Product>.app.tar.gz` plus a `.sig` beside it) but not
// the manifest the plugin polls: that is `latest.json`, which release.yml
// builds here and attaches to the GitHub release next to the .dmg.
//
// The archive is copied to a name without spaces that carries the version,
// so the URL written into the manifest is the URL GitHub serves (GitHub
// rewrites spaces in asset names) and two releases never share a filename.
//
// Usage: node scripts/updater-manifest.mjs <tag> <bundle-dir> <out-dir> <arch>
//   tag         the git tag, `v0.3.0`
//   bundle-dir  src-tauri/target/release/bundle/macos
//   out-dir     where the renamed archive, its .sig and latest.json go
//   arch        `aarch64` or `x86_64`: the platform key the manifest is for
//
// The arch is named rather than taken from whatever the runner happens to be.
// It used to be `process.arch`, which is right today and would have been
// silently wrong the day the runner image changed: every install would poll a
// manifest with no key matching its own Mac and answer "no build for this
// Mac". Named and cross-checked against the runner, a mismatch fails the
// release instead.

import { copyFile, mkdir, readFile, readdir, writeFile } from "node:fs/promises";
import { join } from "node:path";

/** The repository the release assets and the update feed live under. */
export const REPO = "jtn0123/draft-assistant";
const ARCHIVE = ".app.tar.gz";

/**
 * Filename the archive is published under.
 *
 * @param {string} tag `v0.3.0`
 * @param {string} arch `aarch64`
 */
export function assetName(tag, arch) {
  return `Draft-Assistant-${tag.replace(/^v/, "")}-${arch}${ARCHIVE}`;
}

/**
 * The manifest body. `version` is bare, `signature` is the .sig file's whole
 * content, and the platform key is what the plugin sends for Apple Silicon.
 *
 * @param {{ tag: string, arch: string, signature: string, pubDate: string }} input
 */
export function manifest({ tag, arch, signature, pubDate }) {
  return {
    version: tag.replace(/^v/, ""),
    notes: `Release ${tag}. Notes: https://github.com/${REPO}/releases/tag/${tag}`,
    pub_date: pubDate,
    platforms: {
      [platformFor(arch)]: {
        signature: signature.trim(),
        url: `https://github.com/${REPO}/releases/download/${tag}/${assetName(tag, arch)}`,
      },
    },
  };
}

/** The platform keys the app is ever built for, by the arch that names them. */
const PLATFORMS = { aarch64: "darwin-aarch64", x86_64: "darwin-x86_64" };

/**
 * The `platforms` key for `arch`. Throws on anything not in the table: a
 * manifest under a key the plugin never asks for is a manifest no install can
 * use, and it is better not to write one at all.
 *
 * @param {string} arch `aarch64` or `x86_64`
 */
export function platformFor(arch) {
  if (!Object.hasOwn(PLATFORMS, arch)) {
    throw new Error(
      `unknown arch ${arch}; the app is built for: ${Object.keys(PLATFORMS).join(", ")}`,
    );
  }
  return PLATFORMS[arch];
}

/**
 * The `platforms` key for `arch`, checked against the machine that built the
 * bundle. A complaint rather than a throw, so the CLI can print it and stop.
 *
 * @param {string | undefined} arch what the caller asked for
 * @param {string} runner `process.arch` of the machine that ran the build
 * @returns {{ key: string, arch: string } | { error: string }}
 */
export function platformKey(arch, runner) {
  const known = Object.keys(PLATFORMS);
  if (!arch) {
    return { error: `name the arch to build the manifest for, one of: ${known.join(", ")}` };
  }
  if (!Object.hasOwn(PLATFORMS, arch)) {
    return { error: `unknown arch ${arch}; the app is built for: ${known.join(", ")}` };
  }
  const built = runner === "arm64" ? "aarch64" : "x86_64";
  if (built !== arch) {
    return {
      error: `asked for a ${arch} manifest but the bundle was built on ${built}; every install would be told there is no build for this Mac`,
    };
  }
  return { key: PLATFORMS[arch], arch };
}

/**
 * Every file a release needs before it is worth publishing, and what is
 * missing from `names`.
 *
 * The updater archive, its signature and the manifest go up together or the
 * release is not published at all: an install polling
 * `releases/latest/download/latest.json` gets a 404 the moment a release
 * without one becomes the latest, and "Check for updates" answers "No release
 * feed yet" from then on.
 *
 * @param {string[]} names what is in the out dir
 * @param {string} tag `v0.3.0`
 * @param {string} arch `aarch64`
 * @returns {string[]} the missing names, empty when the release is complete
 */
export function missingAssets(names, tag, arch) {
  const asset = assetName(tag, arch);
  return [asset, `${asset}.sig`, "latest.json"].filter((want) => !names.includes(want));
}

/**
 * The one archive in the bundle dir, or a complaint naming what was found.
 *
 * @param {string[]} names
 * @returns {{ archive: string } | { error: string }}
 */
export function pickArchive(names) {
  const archives = names.filter((n) => n.endsWith(ARCHIVE));
  if (archives.length !== 1) {
    return {
      error: `expected one ${ARCHIVE} in the bundle dir, found: ${names.join(", ") || "nothing"}`,
    };
  }
  const archive = archives[0];
  if (!names.includes(`${archive}.sig`)) {
    return {
      error: `${archive} has no .sig beside it; was TAURI_SIGNING_PRIVATE_KEY set for the build?`,
    };
  }
  return { archive };
}

async function main() {
  const [tag, bundleDir, outDir, wanted] = process.argv.slice(2);
  if (!tag || !bundleDir || !outDir) {
    console.error("usage: node scripts/updater-manifest.mjs <tag> <bundle-dir> <out-dir> <arch>");
    process.exit(2);
  }
  const platform = platformKey(wanted, process.arch);
  if ("error" in platform) {
    console.error(platform.error);
    process.exit(1);
  }
  const { arch } = platform;
  const picked = pickArchive(await readdir(bundleDir));
  if ("error" in picked) {
    console.error(picked.error);
    process.exit(1);
  }
  await mkdir(outDir, { recursive: true });
  const name = assetName(tag, arch);
  await copyFile(join(bundleDir, picked.archive), join(outDir, name));
  const signature = await readFile(join(bundleDir, `${picked.archive}.sig`), "utf8");
  await writeFile(join(outDir, `${name}.sig`), signature);
  const body = manifest({ tag, arch, signature, pubDate: new Date().toISOString() });
  await writeFile(join(outDir, "latest.json"), `${JSON.stringify(body, null, 2)}\n`);
  // What was published is checked rather than assumed: the release step
  // behind this refuses to publish an incomplete set, and this is where an
  // incomplete set is noticed.
  const missing = missingAssets(await readdir(outDir), tag, arch);
  if (missing.length > 0) {
    console.error(`the release would ship without: ${missing.join(", ")}`);
    process.exit(1);
  }
  console.log(`wrote ${outDir}/latest.json pointing at ${body.platforms[platform.key].url}`);
}

// Only the CLI touches the filesystem, so the test can import the pure parts.
if (process.argv[1] && import.meta.url === new URL(`file://${process.argv[1]}`).href) {
  await main();
}
