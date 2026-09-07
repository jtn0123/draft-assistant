import { readdir, readFile, stat } from "node:fs/promises";
import { relative, resolve, sep } from "node:path";

export const MAX_LINES = 500;
// The cap is about keeping source readable. Generated audit reports and
// coverage output are neither source nor hand-maintained.
export const EXCLUDED_DIRECTORIES = new Set([
  ".Codex",
  ".claude",
  ".git",
  "coverage",
  "dist",
  "gen",
  "icons",
  "node_modules",
  "research",
  "target",
]);
export const EXCLUDED_FILES = new Set([
  "Cargo.lock",
  "package-lock.json",
  `draft-assistant${sep}public${sep}dev-fixture.json`,
  `draft-assistant${sep}public${sep}dev-season-fixture.json`,
]);

/**
 * True when a directory entry is generated, vendored, or otherwise not
 * first-party source. `entry` is the bare name; `repositoryPath` is the path
 * from the repository root, which is how the two big fixtures are named.
 */
export function isExcluded(entry, repositoryPath) {
  return (
    EXCLUDED_DIRECTORIES.has(entry) ||
    EXCLUDED_FILES.has(entry) ||
    EXCLUDED_FILES.has(repositoryPath)
  );
}

/**
 * Lines in one file, or null when it holds a NUL byte: that is a binary, and
 * counting its "lines" would mean nothing. Takes the file's bytes.
 */
export function lineCount(content) {
  if (content.includes(0)) return null;
  return content.length === 0 ? 0 : content.toString("utf8").split(/\r?\n/).length;
}

/**
 * The files over the cap, each named with its count. `files` is a list of
 * [path, bytes] pairs; a file exactly at the cap is fine.
 */
export function oversized(files) {
  const over = [];
  for (const [path, content] of files) {
    const lines = lineCount(content);
    if (lines !== null && lines > MAX_LINES) over.push([path, lines]);
  }
  return over;
}

async function filesUnder(directory, repositoryRoot) {
  const files = [];
  for (const entry of await readdir(directory)) {
    const path = resolve(directory, entry);
    if (isExcluded(entry, relative(repositoryRoot, path))) continue;
    const metadata = await stat(path);
    if (metadata.isDirectory()) files.push(...(await filesUnder(path, repositoryRoot)));
    else files.push(path);
  }
  return files;
}

async function main() {
  const repositoryRoot = resolve(process.cwd(), "..");
  const read = [];
  // One at a time: the tree holds thousands of files, and reading them all at
  // once would open thousands of descriptors for no gain.
  for (const path of await filesUnder(repositoryRoot, repositoryRoot)) {
    read.push([relative(repositoryRoot, path), await readFile(path)]);
  }

  const over = oversized(read);
  if (over.length > 0) {
    for (const [path, lines] of over) console.error(`${path}: ${lines} lines`);
    process.exitCode = 1;
  } else {
    console.log(`All first-party non-generated files are ${MAX_LINES} lines or fewer.`);
  }
}

// Only the CLI touches the filesystem, so the test can import the pure parts.
if (process.argv[1] && import.meta.url === new URL(`file://${process.argv[1]}`).href) {
  await main();
}
