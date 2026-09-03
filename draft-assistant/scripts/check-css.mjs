// Guards the one real hazard of shipping ten global stylesheets: the same
// class getting rules in two different files, where neither author can see
// the other. Whichever loads last silently wins.
//
// Cheaper and far less disruptive than converting to CSS Modules, which would
// break the deliberate cross-file selectors this codebase relies on.

import { readdir, readFile } from "node:fs/promises";

const dir = new URL("../src/", import.meta.url);
const files = (await readdir(dir)).filter((f) => f.endsWith(".css"));

/** class name -> files that open a rule block starting with it */
const owners = new Map();

for (const file of files) {
  const css = (await readFile(new URL(file, dir), "utf8")).replace(/\/\*[\s\S]*?\*\//g, "");
  // `{` is allowed before a selector so the first rule inside an at-rule
  // block (`@media (...) {`) is seen like every other one.
  for (const match of css.matchAll(/(?:^|[{};])\s*([^{};]+?)\s*\{/g)) {
    // A selector list can hold several rules; each one is owned by the first
    // class in it, so a theme override like `[data-theme="dark"] .pill` is
    // charged to `.pill` rather than slipping past the check.
    for (const selector of match[1].split(",")) {
      const owner = selector.trim().match(/\.(-?[_a-zA-Z][\w-]*)/);
      if (owner === null) continue;
      if (!owners.has(owner[1])) owners.set(owner[1], new Set());
      owners.get(owner[1]).add(file);
    }
  }
}

const shared = [...owners]
  .filter(([, where]) => where.size > 1)
  .sort(([a], [b]) => a.localeCompare(b));

if (shared.length > 0) {
  console.error("These classes are styled from more than one stylesheet:\n");
  for (const [name, where] of shared) {
    console.error(`  .${name} — ${[...where].sort().join(", ")}`);
  }
  console.error(
    "\nKeep every rule for a class in the file that owns it, so nobody has to " +
      "read all ten sheets to know what a class does.",
  );
  process.exitCode = 1;
} else {
  console.log(`No class is styled from more than one of ${files.length} stylesheets.`);
}
