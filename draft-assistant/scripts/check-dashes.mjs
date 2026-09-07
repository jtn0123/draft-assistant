// Guards the house style for user-visible text: no em-dash (U+2014) or
// en-dash (U+2013) in code, only in comments. A dash in a string literal
// ends up in a toast, a table cell, a log line, or a chat prompt, and the
// rule is to write those with a comma, colon, or period instead.
//
// Comments are stripped with a small per-line scanner that knows about
// string literals, so `"https://..."` is not mistaken for a line comment and
// a doc comment with a dash in it does not trip the check.

import { readdir, readFile } from "node:fs/promises";
import { relative, resolve } from "node:path";

const root = resolve(process.cwd());
const trees = ["src", "src-tauri/src", "src-tauri/companion-static"];
const extensions = new Set([".ts", ".tsx", ".rs", ".js", ".mjs", ".html"]);
// The raw characters and their escaped spellings; `\u{2014}` in Rust, `\u2014` in JS.
const dash = /[–—]|\\u\{?201[34]\}?/;

/**
 * Files allowed to keep a dash, each with the reason. Every entry so far is a
 * test that asserts the character never reaches the user, which means naming
 * it once. The scanner's own tests live in `scripts/`, outside the trees.
 */
const allowed = new Map([
  ["src/App.settings.test.tsx", "asserts an empty rejection leaves no dangling dash"],
  ["src/apiRemote.test.ts", "asserts remote error copy is two sentences, not a dash"],
  ["src/updateRow.test.ts", "asserts the update row never renders a dash"],
  ["src-tauri/src/commands_update_tests.rs", "asserts updater errors never carry a dash"],
  ["src-tauri/src/recommend_tests.rs", "asserts recommendation reasons never carry a dash"],
]);

async function filesUnder(directory) {
  const files = [];
  for (const entry of await readdir(directory, { withFileTypes: true })) {
    const path = resolve(directory, entry.name);
    if (entry.isDirectory()) files.push(...(await filesUnder(path)));
    else if ([...extensions].some((ext) => entry.name.endsWith(ext))) files.push(path);
  }
  return files;
}

/**
 * Strips comments from one source line, carrying block-comment state across
 * lines. Returns the code that remains and whether a block comment is open.
 */
export function stripComments(line, state, kind) {
  let out = "";
  let i = 0;
  let inBlock = state.inBlock;
  let quote = null;
  const openBlock = kind === "html" ? "<!--" : "/*";
  const closeBlock = kind === "html" ? "-->" : "*/";
  while (i < line.length) {
    if (inBlock) {
      const end = line.indexOf(closeBlock, i);
      if (end === -1) return { code: out, inBlock: true };
      i = end + closeBlock.length;
      inBlock = false;
      continue;
    }
    const ch = line[i];
    if (quote !== null) {
      out += ch;
      if (ch === "\\") {
        out += line[i + 1] ?? "";
        i += 2;
        continue;
      }
      if (ch === quote) quote = null;
      i += 1;
      continue;
    }
    if (line.startsWith(openBlock, i)) {
      inBlock = true;
      i += openBlock.length;
      continue;
    }
    if (kind !== "html" && line.startsWith("//", i)) return { code: out, inBlock: false };
    // Rust lifetimes and char literals both start with an apostrophe; only
    // JS/TS treat it as a string delimiter. A Rust char literal holding a
    // dash still surfaces because the character itself stays in `out`.
    if (ch === '"' || ch === "`" || (ch === "'" && kind === "js")) quote = ch;
    out += ch;
    i += 1;
  }
  return { code: out, inBlock };
}

export function findDashes(source, kind) {
  const hits = [];
  let state = { inBlock: false };
  source.split(/\r?\n/).forEach((line, index) => {
    const result = stripComments(line, state, kind);
    state = { inBlock: result.inBlock };
    if (dash.test(result.code)) hits.push(index + 1);
  });
  return hits;
}

function kindOf(path) {
  if (path.endsWith(".html")) return "html";
  if (path.endsWith(".rs")) return "rust";
  return "js";
}

export async function scan(base = root) {
  const findings = [];
  for (const tree of trees) {
    for (const path of await filesUnder(resolve(base, tree))) {
      const relativePath = relative(base, path);
      if (allowed.has(relativePath)) continue;
      const source = await readFile(path, "utf8");
      for (const line of findDashes(source, kindOf(path))) {
        findings.push(`${relativePath}:${line}`);
      }
    }
  }
  return findings;
}

if (process.argv[1] && resolve(process.argv[1]) === new URL(import.meta.url).pathname) {
  const findings = await scan();
  if (findings.length > 0) {
    console.error(
      "Em-dash or en-dash in code (comments are fine). Rewrite with a comma, colon, or period:\n",
    );
    for (const hit of findings) console.error(`  ${hit}`);
    console.error(`\n${findings.length} line(s).`);
    process.exitCode = 1;
  } else {
    console.log("No em-dashes or en-dashes outside comments.");
  }
}
