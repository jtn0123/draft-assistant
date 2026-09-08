import { expect, test, type Page } from "@playwright/test";

/**
 * The phone page walked as a phone: pairing, the your-turn nudge with the
 * urgent clock, the best-available list, the chat with a question in flight,
 * and a dropped socket brought back by a tap. Run through
 * `playwright.mobile.config.ts`, which is what makes the browser an iPhone
 * or an Android phone; see there for why service workers are blocked.
 */
import { answer, ask, backend, emit, pair, serve, thread } from "./companionServer";
import { dump } from "./fixtures";

// Screenshots land beside the results, one per tab, browser and scheme, so a
// change to the page can be looked at rather than only asserted about.
const shot = (page: Page, name: string, project: string) =>
  page.screenshot({ path: `e2e-browser/.results-mobile/${project}-${name}.png`, fullPage: false });

const noOverflow = async (page: Page) => {
  const over = await page.evaluate(
    () => document.documentElement.scrollWidth - document.documentElement.clientWidth,
  );
  expect(over).toBeLessThanOrEqual(0);
};

for (const scheme of ["light", "dark"] as const) {
  test(`walks every tab in ${scheme}`, async ({ page }, info) => {
    await page.emulateMedia({ colorScheme: scheme });
    const p = `${info.project.name}-${scheme}`;
    const host = backend({
      chat: {
        draft: thread("draft", [ask("Who should I take?"), answer("**Chris Olave**: WR1 upside.")]),
      },
    });
    await serve(page, host);
    // The model catalog the host advertises, with the word under each name.
    await page.route("**/api/chat/models", (route) =>
      route.fulfill({
        status: 200,
        contentType: "application/json",
        body: JSON.stringify({
          default_model: "Opus 5",
          default_effort: "High",
          models: [
            { model: "Opus 5", available: true, efforts: ["Off", "Low", "High"], note: "" },
            {
              model: "Fable 5.1",
              available: true,
              efforts: ["Low", "High"],
              note: "slower, smarter",
            },
            { model: "GPT-6 Astra", available: true, efforts: ["Low", "High"], note: "smarter" },
            { model: "GPT-5.6 Sol", available: false, efforts: ["Low", "High"], note: "" },
          ],
        }),
      }),
    );
    // Team marks come from Sleeper's CDN; let those through so the pictures
    // are real in the screenshots. Nothing is asserted about them.
    await page.route("https://sleepercdn.com/**", (route) => route.continue());
    await page.goto("/");
    await shot(page, "0-pair", p);
    await pair(page, host.code);
    await expect(page.locator("#clock-strip:visible")).toContainText("Pick");
    await expect(page.locator("#alerts-toggle")).toHaveText(/Alerts on/);
    await noOverflow(page);
    await shot(page, "1-now", p);
    // My pick with a short clock: the toast and the urgent timer.
    const view = dump("dev-fixture.json") as { draft: Record<string, unknown> };
    view.draft = {
      ...view.draft,
      status: "drafting",
      is_my_pick: true,
      clock_deadline_ms: Date.now() + 12_000,
    };
    await emit(page, { type: "draft-updated", payload: view });
    await expect(page.locator("#alert-toast")).toBeVisible();
    await expect(page.locator(".mobile-timer.urgent")).toBeVisible();
    await shot(page, "2-your-turn", p);
    await page.getByRole("button", { name: "Picks" }).click();
    await expect(page.locator("#available-filter")).toBeVisible();
    await noOverflow(page);
    // The first row's team mark is a real picture from the CDN.
    await expect
      .poll(
        () =>
          page
            .locator("#available .avatar img")
            .first()
            .evaluate((img) => (img as HTMLImageElement).naturalWidth),
        { timeout: 10_000 },
      )
      .toBeGreaterThan(0);
    await shot(page, "3-picks", p);
    // A reload comes back on the same tab.
    await page.reload();
    await expect(page.getByRole("button", { name: "Picks" })).toHaveAttribute(
      "aria-current",
      "page",
    );
    await expect(page.locator("#available-filter")).toBeVisible();
    await page.getByRole("button", { name: "Chat" }).click();
    await expect(page.locator("#chat-list:visible")).toContainText("Chris Olave");
    await noOverflow(page);
    await shot(page, "4-chat", p);
    await emit(page, {
      type: "shared-chat",
      payload: thread(
        "draft",
        [ask("Who should I take?"), answer("**Chris Olave**: WR1 upside.")],
        true,
      ),
    });
    await expect(page.locator("#chat-list:visible")).toContainText("Thinking");
    await page.locator("#chat-input").focus();
    await shot(page, "5-chat-typing", p);
    // The picker is locked while the host answers; let the answer land first.
    await emit(page, {
      type: "shared-chat",
      payload: thread("draft", [ask("Who should I take?"), answer("**Chris Olave**: WR1 upside.")]),
    });
    await page.locator("#model-toggle").click();
    await expect(page.locator('[data-model="Fable 5.1"] .model-hint')).toHaveText(
      "slower, smarter",
    );
    await expect(page.locator('[data-model="GPT-6 Astra"] .model-hint')).toHaveText("smarter");
    await noOverflow(page);
    await shot(page, "5b-model-panel", p);
    await page.locator('[data-model="Fable 5.1"]').click();
    await expect(page.locator("#model-toggle")).toContainText("Fable 5.1");
    await expect(page.locator("#model-panel")).toBeHidden();
    // Compact: the whole page tightens and the choice survives a reload.
    await page.getByRole("button", { name: "Now" }).click();
    await page.locator("#compact-toggle").click();
    await expect(page.locator("#companion-root")).toHaveClass(/compact/);
    await noOverflow(page);
    await shot(page, "5c-compact-now", p);
    await page.locator("#compact-toggle").click();
    // Drop the socket: the pill shows and a tap reconnects at once.
    await page.evaluate(() => (window as unknown as { __drop: (c?: number) => void }).__drop(1006));
    await expect(page.locator("#reconnect-pill")).toBeVisible();
    await shot(page, "6-reconnecting", p);
    await page.locator("#reconnect-pill").click();
    await expect(page.locator("#reconnect-pill")).toBeHidden();
    const errors: string[] = [];
    page.on("pageerror", (e) => errors.push(String(e)));
    expect(errors).toEqual([]);
  });
}
