import { browser, $, expect } from "@wdio/globals";
import { resolve } from "node:path";

const host = process.env.REHEARSAL_URL!;
const pick = (id: string, n: number, slot: number) => ({
  player_id: id,
  pick_no: n,
  round: 1,
  draft_slot: slot,
});
async function control(value: unknown) {
  const response = await fetch(`${host}/control`, { method: "POST", body: JSON.stringify(value) });
  if (!response.ok) throw new Error("fixture control failed");
}
async function shot(name: string) {
  await browser.saveScreenshot(resolve(process.env.REHEARSAL_ARTIFACTS!, `${name}.png`));
}
const row = (name: string) => $(`.board-body*=${name}`);

describe("native deterministic draft rehearsal", () => {
  it("loads, follows API picks, records and undoes a manual pick, recovers from an outage", async () => {
    // Explicit standard WebDriver selection disables the service's optional
    // multi-window focus probe, which needs a global Tauri API this app omits.
    const handles = await browser.getWindowHandles();
    await browser.switchToWindow(handles[0]!);
    await $(".card-screen-submit").waitForDisplayed();
    await $('input[placeholder="mcsleeper26"]').setValue("rehearsal");
    await $('input[placeholder="1389710366300200960"]').setValue("1000000000000000001");
    await $(".card-screen-submit").click();
    await expect($(".app-header h1")).toHaveText("Native Rehearsal League");
    await $(".mode-toggle button:nth-child(2)").click();
    await expect(row("Rehearsal Runner")).toBeDisplayed();
    await expect(row("Rehearsal Catcher")).toBeDisplayed();
    await shot("01-loaded");

    await control({ picks: [pick("rb-1", 1, 1)] });
    await browser.waitUntil(async () => !(await row("Rehearsal Runner").isExisting()), {
      timeout: 20_000,
    });
    await expect($(".app")).toHaveText(expect.stringContaining("Rehearsal Runner"));
    await shot("02-api-pick");

    await row("Rehearsal Catcher").$("button.btn-row").click();
    await $('[role="dialog"] .btn-primary').click();
    await browser.waitUntil(async () => !(await row("Rehearsal Catcher").isExisting()));
    await shot("03-manual-pick");
    await $('button[title="Undo last recorded pick"]').click();
    await expect(row("Rehearsal Catcher")).toBeDisplayed();
    await shot("04-undone");

    await control({ outage: true });
    await browser.waitUntil(
      async () => /Sync retrying|Sync stale/.test(await $(".app-header").getText()),
      { timeout: 25_000 },
    );
    await expect(row("Rehearsal Runner")).not.toExist();
    await shot("05-outage");
    await control({ outage: false, picks: [pick("rb-1", 1, 1), pick("wr-1", 2, 2)] });
    await browser.waitUntil(async () => !(await row("Rehearsal Catcher").isExisting()), {
      timeout: 35_000,
    });
    await browser.waitUntil(async () => (await $(".app-header").getText()).includes("Live ·"));
    await shot("06-recovered");
  });
});
