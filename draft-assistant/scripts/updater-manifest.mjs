// `tauri build` with `createUpdaterArtifacts` writes the signed archive the
// updater installs (`<Product>.app.tar.gz` plus a `.sig` beside it) but not
// the manifest the plugin polls: that is `latest.json`, which release.yml
// builds here and attaches to the GitHub release next to the .dmg.
//
// The archive is copied to a name without spaces that carries the version,
// so the URL written into the manifest is the URL GitHub serves (GitHub
// rewrites spaces in asset names) and two releases never share a filename.
//
// Usage: node scripts/updater-manifest.mjs <tag> <bundle-dir> <out-dir>
//   tag         the git tag, `v0.3.0`
//   bundle-dir  src-tauri/target/release/bundle/macos
//   out-dir     where the renamed archive, its .sig and latest.json go

import { copyFile, mkdir, readFile, readdir, writeFile } from "node:fs/promises";
import { join } from "node:path";

const REPO = "jtn0123/draft-assistant";
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
      [`darwin-${arch}`]: {
        signature: signature.trim(),
        url: `https://github.com/${REPO}/releases/download/${tag}/${assetName(tag, arch)}`,
      },
    },
  };
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
  const [tag, bundleDir, outDir] = process.argv.slice(2);
  if (!tag || !bundleDir || !outDir) {
    console.error("usage: node scripts/updater-manifest.mjs <tag> <bundle-dir> <out-dir>");
    process.exit(2);
  }
  const arch = process.arch === "arm64" ? "aarch64" : "x86_64";
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
  console.log(`wrote ${outDir}/latest.json pointing at ${body.platforms[`darwin-${arch}`].url}`);
}

// Only the CLI touches the filesystem, so the test can import the pure parts.
if (process.argv[1] && import.meta.url === new URL(`file://${process.argv[1]}`).href) {
  await main();
}
