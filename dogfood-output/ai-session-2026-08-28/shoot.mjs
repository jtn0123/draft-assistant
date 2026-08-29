// Screenshots of an Ask Claude session in the browser preview, replaying
// tonight's real state (ai-state.json) and the real answers recorded against
// it by `dump_state --chat-out` (ai-session.json). Both are copied into
// public/ for the run and removed afterwards.
//
//   bun ../dogfood-output/ai-session-2026-08-28/shoot.mjs   (from draft-assistant/)
//
// Needs the Vite dev server on :1420.
import { chromium } from "@playwright/test";
import { copyFile, mkdir, readFile, rm, writeFile } from "node:fs/promises";
import { gunzipSync } from "node:zlib";
import { dirname, join } from "node:path";
import { fileURLToPath } from "node:url";

const here = dirname(fileURLToPath(import.meta.url));
const root = join(here, "..", "..", "draft-assistant");
const shots = join(here, "screenshots");
await mkdir(shots, { recursive: true });
// state.json is kept gzipped (500-line cap on repo files); unpack it for the run.
await writeFile(join(root, "public", "ai-state.json"), gunzipSync(await readFile(join(here, "state.json.gz"))));
await copyFile(join(here, "session.json"), join(root, "public", "ai-session.json"));

const browser = await chromium.launch();
const page = await browser.newPage({ viewport: { width: 1500, height: 950 }, deviceScaleFactor: 1 });
page.setDefaultTimeout(15_000);
let n = 0;
const shot = async (name, options = {}) => {
  n += 1;
  const file = join(shots, `${String(n).padStart(2, "0")}-${name}.png`);
  await page.screenshot({ path: file, ...options });
  console.log(file);
};

try {
  await page.goto("http://localhost:1420/?replay=/ai-state.json&chat=/ai-session.json");
  await page.getByRole("heading", { name: "UMass Wrestling Fantasy Football League" }).waitFor();
  await page.getByRole("button", { name: "Ask Claude" }).click();
  const panel = page.getByRole("complementary", { name: /Ask Claude about this draft/ });
  await panel.waitFor();
  await shot("panel-open");

  await panel.locator(".chat-settings summary").click();
  await shot("settings", { clip: { x: 1500 - 380, y: 0, width: 380, height: 950 } });
  await panel.locator(".chat-settings summary").click();

  // Question 1 from a suggestion; catch it mid-stream, then settled.
  await page.getByRole("button", { name: "Who should I take next?" }).click();
  await panel.locator(".chat-turn.streaming").waitFor();
  await shot("streaming", { clip: { x: 1500 - 380, y: 0, width: 380, height: 950 } });
  await panel.locator(".chat-turn.streaming").waitFor({ state: "detached" });
  await shot("answer-1");

  const ask = async (text) => {
    await page.getByLabel("Your question").fill(text);
    await page.keyboard.press("Enter");
    await panel.locator(".chat-turn.streaming").waitFor();
    await panel.locator(".chat-turn.streaming").waitFor({ state: "detached" });
  };
  await ask("Who is likely gone before my next pick at 27?");
  await shot("answer-2", { clip: { x: 1500 - 380, y: 0, width: 380, height: 950 } });
  await ask("Plan my next three picks.");
  await shot("answer-3", { clip: { x: 1500 - 380, y: 0, width: 380, height: 950 } });
  await shot("session-full");

  // Two picks land after the answers were written: the replay source is
  // advanced and the stamps under the answers must say so.
  const state = JSON.parse(gunzipSync(await readFile(join(here, "state.json.gz"))).toString("utf8"));
  state.generated_at += 1;
  state.draft.current_pick = 3;
  state.draft.current_round = 1;
  await writeFile(join(root, "public", "ai-state.json"), JSON.stringify(state));
  await panel.locator(".chat-asof.stale").first().waitFor();
  await shot("stale-stamp", { clip: { x: 1500 - 380, y: 0, width: 380, height: 950 } });
} finally {
  await browser.close();
  await rm(join(root, "public", "ai-state.json"), { force: true });
  await rm(join(root, "public", "ai-session.json"), { force: true });
}
