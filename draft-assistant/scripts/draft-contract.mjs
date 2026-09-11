// Cargo parses the Rust contract; Prettier makes its checked-in output stable.
import { spawnSync } from "node:child_process";
import { readFile, writeFile } from "node:fs/promises";
import { format } from "prettier";

const result = spawnSync(
  "cargo",
  ["run", "--quiet", "--manifest-path", "src-tauri/Cargo.toml", "--example", "draft_contract"],
  { encoding: "utf8", maxBuffer: 4 * 1024 * 1024 },
);
if (result.error || result.status !== 0) {
  throw new Error(`Contract generation failed: ${result.error?.message ?? result.stderr}`);
}
const path = "src/draft-contract.generated.ts";
const expected = await format(result.stdout, { parser: "typescript", printWidth: 100 });
if (process.argv.includes("--write")) {
  await writeFile(path, expected);
  console.log(`Generated ${path} from the Rust wire contract.`);
} else {
  const actual = await readFile(path, "utf8").catch(() => "");
  if (actual !== expected) {
    throw new Error(`Draft contract is stale. Run npm run generate:contract and commit ${path}.`);
  }
  console.log("Draft DTOs and schema versions match Rust.");
}
