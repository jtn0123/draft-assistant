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
