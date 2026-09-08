import { spawn } from "node:child_process";
import { mkdtemp, mkdir, symlink, unlink, writeFile } from "node:fs/promises";
import { openSync, closeSync } from "node:fs";
import { tmpdir, homedir } from "node:os";
import { resolve, join } from "node:path";
import { fileURLToPath } from "node:url";
import { startFixture } from "./rehearsal-fixture.mjs";

if (process.platform !== "darwin")
  throw new Error("Native rehearsal currently targets macOS WKWebView");
const root = resolve(fileURLToPath(new URL("..", import.meta.url)));
const artifacts = await mkdtemp(join(tmpdir(), "draft-native-rehearsal-"));
const data = join(artifacts, "data");
const link = join(homedir(), "Library/Application Support/com.justin.draft-assistant.rehearsal");
await mkdir(data);
// Refuse an existing rehearsal profile; never replace a real or previous profile.
await symlink(data, link);
const fixture = await startFixture();
const env = {
  ...process.env,
  DRAFT_ASSISTANT_SLEEPER_BASE: fixture.url,
  REHEARSAL_URL: fixture.url,
  REHEARSAL_ARTIFACTS: artifacts,
  HTTPS_PROXY: fixture.url,
  HTTP_PROXY: fixture.url,
  ALL_PROXY: fixture.url,
  NO_PROXY: "localhost,127.0.0.1,::1",
};
async function run(args, log, childEnv = process.env) {
  const fd = openSync(join(artifacts, log), "w");
  try {
    await new Promise((resolve, reject) => {
      const child = spawn("npm", args, { cwd: root, env: childEnv, stdio: ["ignore", fd, fd] });
      child.on("error", reject);
      child.on("exit", (code) =>
        code === 0
          ? resolve()
          : reject(new Error(`${args.join(" ")} failed (${code}); see ${artifacts}/${log}`)),
      );
    });
  } finally {
    closeSync(fd);
  }
}
console.log(`Native rehearsal evidence: ${artifacts}`);
try {
  await run(
    [
      "run",
      "tauri",
      "--",
      "build",
      "--features",
      "wdio",
      "--no-bundle",
      "--config",
      JSON.stringify({ identifier: "com.justin.draft-assistant.rehearsal" }),
    ],
    "build.log",
    { ...process.env, CARGO_TARGET_DIR: join(root, "src-tauri/target/wdio") },
  );
  await run(["--prefix", "e2e", "ci"], "install.log");
  await run(["--prefix", "e2e", "run", "test:rehearsal"], "test.log", env);
  console.log(`Native rehearsal passed. Screenshots and logs: ${artifacts}`);
} finally {
  await writeFile(join(artifacts, "requests.json"), JSON.stringify(fixture.requests, null, 2));
  fixture.server.closeAllConnections();
  fixture.server.close();
  await unlink(link);
}
