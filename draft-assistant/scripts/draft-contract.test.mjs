import { test } from "node:test";
import assert from "node:assert/strict";
import { mkdtempSync, mkdirSync, writeFileSync, appendFileSync, rmSync } from "node:fs";
import { tmpdir } from "node:os";
import { join } from "node:path";
import { fileURLToPath } from "node:url";
import { spawnSync } from "node:child_process";

// Exercise the real CLI guard without compiling the application in a script
// unit test. Rust's example tests validate the serializer-to-TS mapping itself.
test("contract check rejects stale output and generator failure", () => {
  const root = mkdtempSync(join(tmpdir(), "draft-contract-test-"));
  try {
    mkdirSync(join(root, "src"));
    const cargo = join(root, "cargo");
    writeFileSync(cargo, '#!/bin/sh\nprintf "export interface DraftView { name: string; }\\n"\n', {
      mode: 0o755,
    });
    const run = (...args) =>
      spawnSync(
        process.execPath,
        [fileURLToPath(new URL("draft-contract.mjs", import.meta.url)), ...args],
        {
          cwd: root,
          env: { ...process.env, PATH: `${root}:${process.env.PATH}` },
          encoding: "utf8",
        },
      );
    assert.equal(run("--write").status, 0);
    assert.equal(run().status, 0);
    appendFileSync(join(root, "src/draft-contract.generated.ts"), "// stale change\n");
    const stale = run();
    assert.notEqual(stale.status, 0);
    assert.match(stale.stderr, /contract is stale/);
    writeFileSync(cargo, "#!/bin/sh\nexit 1\n");
    const failed = run("--write");
    assert.notEqual(failed.status, 0);
    assert.match(failed.stderr, /Contract generation failed/);
  } finally {
    rmSync(root, { recursive: true, force: true });
  }
});
