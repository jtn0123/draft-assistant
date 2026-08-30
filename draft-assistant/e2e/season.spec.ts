import { expect, test } from "@playwright/test";

/**
 * Season-mode E2E against `public/dev-season.json`: a real dump of the
 * league the night the draft completed (board trimmed), replayed through
 * `?replay=`. Numbers are read from the page where they could drift.
 */

test.beforeEach(async ({ page }) => {
  await page.goto("/?replay=/dev-season.json");
  await expect(
    page.getByRole("heading", { name: "UMass Wrestling Fantasy Football League" }),
  ).toBeVisible();
});

test("a complete draft opens on the season screen with the week in the banner", async ({ page }) => {
  await expect(page.getByRole("button", { name: "Season screen" })).toHaveAttribute("aria-pressed", "true");
  const banner = page.getByLabel(/Week \d+ summary/);
  await expect(banner).toBeVisible();
  await expect(banner).toContainText(/vs \S+/);
  await expect(banner).toContainText(/\d+\.\d – \d+\.\d · \d+% to win/);
  await expect(banner).toContainText(/projected \d+(st|nd|rd|th) of 14/);
  // Nothing from the draft cockpit.
  await expect(page.locator(".clock:not(.week-banner)")).toHaveCount(0);
  await expect(page.locator(".board")).toHaveCount(0);
});

test("the lineups table pairs both teams slot by slot and totals match the banner", async ({ page }) => {
  const table = page.getByRole("table", { name: "Lineups side by side" });
  await expect(table).toBeVisible();
  const total = table.locator("tr.total");
  const cells = await total.locator("td.num").allInnerTexts();
  expect(cells).toHaveLength(2);
  const banner = page.getByLabel(/Week \d+ summary/);
  await expect(banner).toContainText(`${cells[0]} – ${cells[1]}`);
  // The dump has a DEF slot nobody fills: the row says so, in red.
  await expect(table.locator("td.empty")).toHaveCount(1);
});

test("the waiver board and trade ideas are on the acting side of the page", async ({ page }) => {
  const waivers = page.getByRole("list", { name: "Waiver targets" });
  await expect(waivers).toBeVisible();
  await expect(waivers.getByRole("listitem").first()).toContainText(/\+\d+/);
  const ideas = page.getByRole("list", { name: "Trade ideas" });
  await expect(ideas.getByRole("listitem").first()).toContainText(/them \+\d+/);
  const lineup = await page.getByRole("heading", { name: "Lineup check" }).boundingBox();
  const standings = await page.getByRole("heading", { name: "Projected standings" }).boundingBox();
  expect(lineup!.x).toBeLessThan(standings!.x);
});

test("the draft screen is still there, read-only, one switch away", async ({ page }) => {
  await page.getByRole("button", { name: "Draft screen" }).click();
  await expect(page.locator(".board")).toBeVisible();
  for (const button of await page.getByRole("button", { name: "Draft", exact: true }).all()) {
    await expect(button).toBeDisabled();
  }
  await expect(page.getByRole("button", { name: "Undo" })).toBeVisible();
});

test("nothing overflows the window at the 1000px minimum width", async ({ page }) => {
  await page.setViewportSize({ width: 1000, height: 900 });
  await expect(page.getByRole("heading", { name: "Lineup check" })).toBeVisible();
  const overflow = await page.evaluate(
    () => document.documentElement.scrollWidth - document.documentElement.clientWidth,
  );
  expect(overflow).toBeLessThanOrEqual(0);
});

test("the header says where the season is, not how big the draft was", async ({ page }) => {
  await expect(page.locator(".brand .muted")).toHaveText(/^\d{4} · Week \d+ · vs \S+$/);
  await page.getByRole("button", { name: "Draft screen" }).click();
  await expect(page.locator(".brand .muted")).toContainText("14 teams · 15 rounds");
});

test("a tap on a name opens a card with what the app knows", async ({ page }) => {
  const table = page.getByRole("table", { name: "Lineups side by side" });
  const first = table.locator("tbody tr").first().locator("button.player-link").first();
  const name = await first.innerText();
  await first.click();
  const card = page.getByRole("dialog", { name });
  await expect(card).toBeVisible();
  await expect(card).toContainText("Owner");
  await expect(card).toContainText("YOU");
  await expect(card).toContainText(/Week \d+/);
  await page.getByRole("button", { name: "Close player card" }).click();
  await expect(card).toHaveCount(0);
});

test("a trade idea fills and prices the offer in one tap", async ({ page }) => {
  const idea = page.getByRole("list", { name: "Trade ideas" }).getByRole("listitem").first();
  await idea.getByRole("button", { name: /^Price / }).click();
  const form = page.locator(".trade-offer");
  await expect(form).toHaveAttribute("open", "");
  // The browser preview cannot price; it says so where the verdict would be.
  await expect(form.locator(".error")).toContainText(/desktop/i);
  await expect(form.getByRole("checkbox", { checked: true })).not.toHaveCount(0);
});

test("draft picks are offerable currency, priced by round", async ({ page }) => {
  // Last season 34 of this league's 38 trades moved a pick (src-tauri/src/pick_value.rs).
  const form = page.locator(".trade-offer");
  await form.getByRole("heading", { name: "Price an offer" }).click();
  const first = form.getByRole("button", { name: /^Round 1 pick, worth \d+ points/ }).first();
  await expect(first).toHaveAttribute("aria-pressed", "false");
  await first.click();
  await expect(first).toHaveAttribute("aria-pressed", "true");
  // A pick alone is an offer: the button is live with no player ticked.
  await expect(form.getByRole("button", { name: "Price it" })).toBeEnabled();
});
