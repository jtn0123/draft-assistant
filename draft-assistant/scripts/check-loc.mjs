import { readdir, readFile, stat } from "node:fs/promises";
import { relative, resolve, sep } from "node:path";

const repositoryRoot = resolve(process.cwd(), "..");
const maxLines = 500;
const excludedDirectories = new Set([
  ".git",
  "dist",
  "gen",
  "icons",
  "node_modules",
  "research",
  "target",
  "corpus",
  "artifacts",
  "playwright-report",
  "test-results",
  // Vitest's v8 HTML report (bun run coverage:frontend); git-ignored.
  "coverage",
]);
const excludedFiles = new Set([
  "Cargo.lock",
  "package-lock.json",
  "bun.lock",
  `draft-assistant${sep}public${sep}dev-fixture.json`,
  // A completed draft in season mode, from a real dump; the e2e season specs.
  `draft-assistant${sep}public${sep}dev-season.json`,
  // Written by scripts/replay-sleeper.mjs --dump; git-ignored.
  `draft-assistant${sep}public${sep}live-state.json`,
]);

async function filesUnder(directory) {
  const files = [];
  for (const entry of await readdir(directory)) {
    if (excludedDirectories.has(entry)) continue;
    const path = resolve(directory, entry);
    const repositoryPath = relative(repositoryRoot, path);
    if (excludedFiles.has(entry) || excludedFiles.has(repositoryPath)) continue;
    const metadata = await stat(path);
    if (metadata.isDirectory()) files.push(...(await filesUnder(path)));
    else files.push(path);
  }
  return files;
}

const oversized = [];
for (const path of await filesUnder(repositoryRoot)) {
  const content = await readFile(path);
  if (content.includes(0)) continue;
  const lines = content.length === 0 ? 0 : content.toString("utf8").split(/\r?\n/).length;
  if (lines > maxLines) oversized.push([relative(repositoryRoot, path), lines]);
}

if (oversized.length > 0) {
  for (const [path, lines] of oversized) console.error(`${path}: ${lines} lines`);
  process.exitCode = 1;
} else {
  console.log(`All first-party non-generated files are ${maxLines} lines or fewer.`);
}
