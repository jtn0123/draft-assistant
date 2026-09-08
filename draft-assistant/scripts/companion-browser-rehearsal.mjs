// Driven by the ignored Rust companion_wire browser test, never a real account.
import { chromium, expect } from "@playwright/test";
import { spawn } from "node:child_process";
import { openSync, closeSync } from "node:fs";
import { createInterface } from "node:readline";
import { join } from "node:path";
const host = process.env.COMPANION_TEST_URL;
const code = process.env.COMPANION_TEST_CODE;
const artifacts = process.env.COMPANION_TEST_ARTIFACTS;
if (!host || !code || !artifacts) throw new Error("Run the ignored companion_wire browser test");
const lines = createInterface({ input: process.stdin })[Symbol.asyncIterator]();
async function signal(step) {
  process.stdout.write(`${step}\n`);
  const line = await lines.next();
  if (line.done) throw new Error("Host fixture stopped");
  return line.value;
}
const frontend = "http://localhost:1420";
const fd = openSync(join(artifacts, "vite.log"), "w");
const vite = spawn(
  process.execPath,
  ["node_modules/vite/bin/vite.js", "--host", "localhost", "--port", "1420", "--strictPort"],
  { stdio: ["ignore", fd, fd] },
);
let browser, phone, desktop;
try {
  await expect
    .poll(
      async () => {
        try {
          return (await fetch(`${frontend}/src/apiRemote.ts`)).ok;
        } catch {
          return false;
        }
      },
      { timeout: 20_000 },
    )
    .toBe(true);
  browser = await chromium.launch();
  const mobile = await browser.newContext({ viewport: { width: 390, height: 844 } });
  const computer = await browser.newContext({ viewport: { width: 1440, height: 1000 } });
  for (const context of [mobile, computer]) {
    await context.route("**/*", async (route) => {
      const url = new URL(route.request().url());
      if (!["localhost", "127.0.0.1"].includes(url.hostname)) return route.abort();
      return route.continue();
    });
  }
  phone = await mobile.newPage();
  phone.setDefaultTimeout(15_000);
  await phone.goto(host);
  await phone.getByLabel("Pairing code").fill(code);
  await phone.getByLabel("This device", { exact: true }).fill("Fixture phone");
  await phone.getByRole("button", { name: "Connect", exact: true }).click();
  await expect(phone.locator("#clock-strip")).toContainText("Pick");
  await phone.screenshot({ path: join(artifacts, "01-phone-paired.png") });

  const desktopCode = await signal("NEXT_CODE");
  desktop = await computer.newPage();
  desktop.setDefaultTimeout(15_000);
  await desktop.goto(frontend);
  await desktop.locator('button[title="Settings"]').click();
  await desktop.getByRole("menuitem", { name: /All settings/ }).click();
  await desktop.getByRole("button", { name: "Join another Draft Assistant…" }).click();
  await desktop.getByLabel("Host address").fill(host);
  await desktop.getByLabel("Code", { exact: true }).fill(desktopCode);
  await desktop.getByLabel("This device’s name").fill("Fixture desktop");
  await desktop.getByRole("button", { name: "Join", exact: true }).click();
  await expect(desktop.locator(".app-header h1")).toHaveText("Fixture League");
  await desktop.locator(".mode-toggle button").filter({ hasText: "Draft" }).click();
  // Follower controls must say who owns drafting before a user can open a
  // confirmation for an action the remote API refuses.
  const recommendations = desktop.getByRole("button", { name: "Mark drafted", exact: true });
  const draftRows = desktop.locator(".board-body button.btn-row");
  await expect(recommendations.first()).toBeVisible();
  await expect(draftRows.first()).toBeVisible();
  for (const button of await recommendations.all()) await expect(button).toBeDisabled();
  for (const button of await draftRows.all()) await expect(button).toBeDisabled();
  await desktop.screenshot({ path: join(artifacts, "02-desktop-paired.png") });
  await signal("PAIRED");

  // Exercise the actual remote API implementation in the desktop frontend.
  const writeError = await desktop.evaluate(async () => {
    const { api } = await import("/src/api.ts");
    try {
      await api.recordManualPick("no-such-fixture-player");
      return "unexpected success";
    } catch (error) {
      return String(error);
    }
  });
  expect(writeError).toContain("controlled by the host");
  const resetStatus = await phone.evaluate(async () => {
    const token = localStorage.getItem("da.companion.token");
    return (
      await fetch("/api/chat/reset", {
        method: "POST",
        headers: {
          authorization: `Bearer ${token}`,
          "content-type": "application/json",
        },
        body: JSON.stringify({ screen: "draft" }),
      })
    ).status;
  });
  expect(resetStatus).toBe(200);

  await mobile.setOffline(true);
  await computer.setOffline(true);
  await signal("DROP");
  await expect(phone.locator("#reconnect-pill")).toBeVisible({ timeout: 15_000 });
  await expect(desktop.locator(".app-header")).toContainText("Reconnecting", { timeout: 15_000 });
  await signal("UPDATE");
  await phone.screenshot({ path: join(artifacts, "03-phone-offline.png") });
  await desktop.screenshot({ path: join(artifacts, "04-desktop-offline.png") });
  await mobile.setOffline(false);
  await computer.setOffline(false);
  await expect(phone.locator("#clock-strip")).toContainText("Recovered Manager", {
    timeout: 30_000,
  });
  await expect(desktop.locator(".app-header h1")).toHaveText("Recovered Fixture League", {
    timeout: 30_000,
  });
  await expect(phone.locator("#reconnect-pill")).toBeHidden();
  await expect(desktop.locator(".app-header")).not.toContainText("Reconnecting");
  await expect(recommendations.first()).toBeDisabled();
  await expect(draftRows.first()).toBeDisabled();
  await phone.screenshot({ path: join(artifacts, "05-phone-recovered.png") });
  await desktop.screenshot({ path: join(artifacts, "06-desktop-recovered.png") });
  await signal("REVOKE");
  await expect(phone.getByRole("button", { name: "Connect", exact: true })).toBeVisible();
  await expect(desktop.locator(".app-header")).toContainText("unpaired this device");
  process.stdout.write("PASSED\n");
} catch (error) {
  for (const [name, page] of [
    ["phone", phone],
    ["desktop", desktop],
  ]) {
    if (page) {
      await page
        .locator('input[inputmode="numeric"]')
        .fill("", { timeout: 500 })
        .catch(() => {});
      await page.screenshot({ path: join(artifacts, `${name}-failure.png`) }).catch(() => {});
    }
  }
  // Error diagnostics may include request URLs; redact any bearer query value.
  process.stderr.write(String(error).replace(/token=[^\s"&]+/g, "token=[redacted]") + "\n");
  process.exitCode = 1;
} finally {
  await browser?.close();
  vite.kill("SIGTERM");
  closeSync(fd);
  process.stdin.destroy();
}
