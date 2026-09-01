import { expect, test } from "@playwright/test";
import { dump, serveReplay } from "./fixtures";

/**
 * The season screen, rendered by a real browser from
 * `public/dev-season-fixture.json`: week 1 of a 14-team league, our matchup
 * projected ahead. The preview opens here, because the season is the everyday
 * screen and the draft is a few hours a year.
 */

test.beforeEach(async ({ page }) => {
  await page.goto("/");
  await expect(
    page.getByRole("heading", { name: "UMass Wrestling Fantasy Football League" }),
  ).toBeVisible();
});

test("opens on the season screen with this week's matchup across the top", async ({ page }) => {
  await expect(page.getByRole("button", { name: "Season", exact: true })).toHaveAttribute(
    "aria-pressed",
    "true",
  );
  await expect(page.locator(".season-stat").first()).toContainText(/vs \S+.* · \d+\.\d – \d+\.\d/);
  await expect(page.getByText("Win odds")).toBeVisible();
  await expect(page.getByText("Playoffs")).toBeVisible();
  // The draft cockpit is not on this screen.
  await expect(page.locator(".board")).toHaveCount(0);
  await expect(page.locator(".clock")).toHaveCount(0);
});

test("the lineup, the calls to make, and the waiver board share the main column", async ({
  page,
}) => {
  await expect(page.getByText("Lineups, slot by slot")).toBeVisible();
  await expect(page.getByText(/points on the table|call to make|calls to make/)).toBeVisible();
  await expect(page.getByText("Worth a claim")).toBeVisible();
  // Both sides of the head-to-head are paired slot by slot.
  await expect(page.getByText("Your player")).toBeVisible();
  await expect(page.getByText("Their player")).toBeVisible();
});

test("the rail tabs swap panels, and arrow keys move within the tablist", async ({ page }) => {
  const rail = page.getByRole("tablist", { name: "League detail" });
  const standings = rail.getByRole("tab", { name: "Standings" });
  await expect(standings).toHaveAttribute("aria-selected", "true");
  await expect(page.getByRole("tabpanel")).toContainText("Proj: best lineup each week");

  await rail.getByRole("tab", { name: "My team" }).click();
  await expect(page.getByRole("tabpanel")).toContainText("starting");

  // Roving tabindex: the tablist takes one Tab stop, arrows move inside it.
  await rail.getByRole("tab", { name: "My team" }).press("ArrowRight");
  await expect(rail.getByRole("tab", { name: "Trends" })).toHaveAttribute("aria-selected", "true");
  await rail.getByRole("tab", { name: "Trends" }).press("Home");
  await expect(standings).toHaveAttribute("aria-selected", "true");
});

test("a standings column header re-sorts the table", async ({ page }) => {
  const seed = page.getByRole("button", { name: /^#, sorted/ });
  await expect(seed).toHaveAccessibleName(/ascending/);
  const first = await page.locator(".standings-row").nth(1).innerText();
  await seed.click();
  await expect(seed).toHaveAccessibleName(/descending/);
  await expect(page.locator(".standings-row").nth(1)).not.toHaveText(first);
});

test("nothing overflows the window at the 1000px minimum width", async ({ page }) => {
  await page.setViewportSize({ width: 1000, height: 900 });
  await expect(page.getByText("Lineups, slot by slot")).toBeVisible();
  const overflow = await page.evaluate(
    () => document.documentElement.scrollWidth - document.documentElement.clientWidth,
  );
  expect(overflow).toBeLessThanOrEqual(0);
});

test("a replay source brings new scores in on its own", async ({ page }) => {
  const season = dump("dev-season-fixture.json");
  const header = season.header as Record<string, number | string>;
  const server = await serveReplay(page, "/live-season.json", season);
  await page.goto("/?replay-season=/live-season.json");
  const banner = page.locator(".season-stat").first();
  await expect(banner).toContainText("135.7");

  server.write({
    ...season,
    generated_at: (season.generated_at as number) + 60,
    header: { ...header, my_projected: 148.25 },
  });
  await expect(banner).toContainText("148.3", { timeout: 15_000 });

  // And live scoring never complained on the way: it is running, not refused.
  // (The draft half was not pointed anywhere, so its own sync still says so.)
  await expect(page.locator(".toast")).not.toContainText("Live updates are not running");
});

test("without a replay source the preview says live scoring needs the desktop app", async ({
  page,
}) => {
  const toast = page.locator(".toast");
  await expect(toast).toContainText(/Live updates are not running/);
  await expect(toast).toContainText(/desktop app/);
});
