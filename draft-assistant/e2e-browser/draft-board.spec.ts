import { expect, test } from "@playwright/test";
import { dump, serveReplay } from "./fixtures";

/**
 * The draft board, rendered by a real browser from `public/dev-fixture.json`:
 * a 14-team full-PPR league in round 2 with our slot on the clock. Anything
 * that moves when the fixture is regenerated is read off the page rather than
 * written down here.
 */

/** The preview opens on the season screen; the board is one click away. */
async function openBoard(page: import("@playwright/test").Page) {
  await expect(
    page.getByRole("heading", { name: "UMass Wrestling Fantasy Football League" }),
  ).toBeVisible();
  await page.getByRole("button", { name: "Draft", exact: true }).click();
  await expect(page.locator(".board")).toBeVisible();
}

test.beforeEach(async ({ page }) => {
  await page.goto("/");
  await openBoard(page);
});

test("renders the draft state the fixture captured", async ({ page }) => {
  await expect(page.locator(".header-meta")).toContainText("14-team");
  await expect(page.locator(".header-subtitle")).toHaveText(/Round \d+ of \d+ · \d+ picks in/);

  // The fixture has us on the clock, with one recommendation, and that player
  // is still on the board below.
  await expect(page.locator(".clock")).toContainText("on the clock");
  await expect(page.locator(".recs .rec")).toHaveCount(1);
  const recommended = await page.locator(".rec-name").first().innerText();
  expect(recommended.length).toBeGreaterThan(0);
  await expect(page.locator(".board")).toContainText(recommended);
});

test("filters the board by position", async ({ page }) => {
  const filters = page.getByRole("group", { name: "Filter players by position" });
  for (const position of ["ALL", "QB", "RB", "WR", "TE", "DEF"]) {
    await expect(filters.getByRole("button", { name: position, exact: true })).toBeVisible();
  }
  await filters.getByRole("button", { name: "QB", exact: true }).click();
  await expect(filters.getByRole("button", { name: "QB", exact: true })).toHaveAttribute(
    "aria-pressed",
    "true",
  );
  // Every badge left on the board reads position + positional rank, e.g. QB2.
  const badges = await page.locator(".board-body .pos-badge").allInnerTexts();
  expect(badges.length).toBeGreaterThan(0);
  for (const badge of badges) expect(badge.replace(/\s+/g, "")).toMatch(/^QB\d+$/);
});

test("search narrows the board, and says so when nothing matches", async ({ page }) => {
  const search = page.getByLabel("Search players");
  const count = page.locator(".board-count");
  const before = await count.innerText();

  await search.fill("zzz-nobody-is-called-this");
  await expect(count).toHaveText("0 players");
  await expect(page.locator(".board")).toContainText("No players match");

  await page.getByRole("button", { name: "Clear filters" }).click();
  await expect(count).toHaveText(before);
});

test("clicking a column header re-sorts the board", async ({ page }) => {
  const points = page.getByRole("button", { name: /^Pts/ });
  await expect(points).toContainText("▼");
  const highest = await page.locator(".board-body").first().innerText();
  await points.click();
  await expect(points).toContainText("▲");
  await expect(page.locator(".board-body").first()).not.toHaveText(highest);
});

test("a replay source keeps the board moving without a reload", async ({ page }) => {
  const state = dump("dev-fixture.json");
  const server = await serveReplay(page, "/live-state.json", state);
  await page.goto("/?replay=/live-state.json");
  await openBoard(page);
  const subtitle = page.locator(".header-subtitle");
  await expect(subtitle).toContainText("26 picks in");

  // The recording moves on: two more picks, and a newer stamp. `generated_at`
  // is what orders the dumps, because each `dump_state` run numbers itself
  // from scratch.
  const draft = state.draft as Record<string, number>;
  server.write({
    ...state,
    generated_at: (state.generated_at as number) + 60,
    draft: { ...draft, total_picks_made: draft.total_picks_made + 2 },
  });

  // No reload, no click: the preview's poll pushes it through the same
  // listener the desktop poller feeds.
  await expect(subtitle).toContainText("28 picks in", { timeout: 15_000 });
});

test("an older dump is ignored, so a restarted recording cannot rewind the board", async ({
  page,
}) => {
  const state = dump("dev-fixture.json");
  const server = await serveReplay(page, "/live-state.json", state);
  await page.goto("/?replay=/live-state.json");
  await openBoard(page);
  const subtitle = page.locator(".header-subtitle");
  await expect(subtitle).toContainText("26 picks in");

  const draft = state.draft as Record<string, number>;
  server.write({
    ...state,
    generated_at: (state.generated_at as number) - 60,
    draft: { ...draft, total_picks_made: 3 },
  });
  // Give the poll several turns to get it wrong.
  await page.waitForTimeout(9000);
  await expect(subtitle).toContainText("26 picks in");
});

test("live sync stays off, and says why, without a replay source", async ({ page }) => {
  await expect(page.locator(".header-actions")).toContainText(/Live sync off/i);
});

test("an injury tag stays inside its column with the chat panel open", async ({ page }) => {
  // The fixture carries Sleeper's own spellings ("Questionable", "IR", "PUP")
  // and the row draws each as one letter. Opening the chat takes 380px out of
  // the player column, which is where a tag that could not shrink used to run
  // straight over the position badge beside it.
  await page.getByRole("button", { name: "Ask Claude" }).click();
  await expect(page.locator(".board")).toBeVisible();

  const rows = page.locator(".board-body").filter({ has: page.locator(".tag") });
  const count = await rows.count();
  expect(count).toBeGreaterThan(0);

  for (let i = 0; i < count; i += 1) {
    const tag = rows.nth(i).locator(".tag");
    // One letter on screen; the spelled-out word rides along for a screen
    // reader, off-screen and out of the layout.
    await expect(tag.locator('[aria-hidden="true"]')).toHaveText(/^[QDO]$/);
    const tagBox = await tag.boundingBox();
    const badgeBox = await rows.nth(i).locator(".pos-badge").boundingBox();
    if (tagBox === null || badgeBox === null) throw new Error("both are on screen");
    // The tag ends before the badge begins: no overlap, at any width.
    expect(tagBox.x + tagBox.width).toBeLessThanOrEqual(badgeBox.x);
  }
});
