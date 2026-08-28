import { expect, test } from "@playwright/test";

/**
 * Browser-preview E2E. The app serves `public/dev-fixture.json` outside Tauri,
 * so these run against a real rendering engine with a real, fixed draft state:
 * a 14-team keeper league mid-draft, our slot on the clock, with one
 * recommendation. Values that move when the fixture is regenerated are read
 * from the page rather than hard-coded.
 */

test.beforeEach(async ({ page }) => {
  await page.goto("/");
  await expect(
    page.getByRole("heading", { name: "UMass Wrestling Fantasy Football League" }),
  ).toBeVisible();
});

test("renders the live draft state from the fixture", async ({ page }) => {
  // Header facts.
  await expect(page.getByText("14 teams", { exact: false })).toBeVisible();

  // Both recommendation modes resolve to the same player in this fixture and
  // App.tsx de-duplicates them, so exactly one recommendation card renders.
  await expect(page.locator(".recs .rec")).toHaveCount(1);
  const recommended = await page.locator(".recs .rec-name").innerText();
  expect(recommended.length).toBeGreaterThan(0);

  // The fixture has us on the clock, and the recommended player is on the board.
  await expect(page.locator(".clock")).toContainText("YOU ARE ON THE CLOCK");
  await expect(page.locator(".board tbody")).toContainText(recommended);
});

test("filters the board by position", async ({ page }) => {
  const board = page.getByRole("group", { name: "Filter players by position" });
  await expect(board).toBeVisible();

  // The league has no kicker slot, so K must not be offered as a filter.
  await expect(board.getByRole("button", { name: "K", exact: true })).toHaveCount(0);
  for (const position of ["QB", "RB", "WR", "TE", "DEF"]) {
    await expect(board.getByRole("button", { name: position, exact: true })).toBeVisible();
  }

  await board.getByRole("button", { name: "QB", exact: true }).click();
  await expect(board.getByRole("button", { name: "QB", exact: true })).toHaveAttribute(
    "aria-pressed",
    "true",
  );

  // Badges read position + positional rank, e.g. "QB1".
  const badges = page.locator("tbody .pos-badge");
  await expect(badges.first()).toBeVisible();
  const texts = await badges.allTextContents();
  expect(texts.length).toBeGreaterThan(0);
  for (const text of texts) {
    expect(text.trim()).toMatch(/^QB\d+$/);
  }
});

test("search narrows the board and shows an empty state", async ({ page }) => {
  const search = page.getByLabel("Search players");

  await search.fill("Waddle");
  await expect(page.locator("tbody tr")).toHaveCount(1);
  await expect(page.locator("tbody")).toContainText("Jaylen Waddle");

  // A search matching nobody must explain itself, not render a blank table.
  await search.fill("zzzzzznotaplayer");
  await expect(page.locator(".empty-board")).toHaveText("No matching players");
  await expect(page.getByText("0 players")).toBeVisible();
});

test("the draft confirmation is reversible", async ({ page }) => {
  await page.getByRole("button", { name: "Draft" }).first().click();

  const confirm = page.getByRole("button", { name: "Confirm" });
  await expect(confirm).toBeVisible();
  await expect(page.getByText(/Mark .* as drafted at pick/)).toBeVisible();

  await page.getByRole("button", { name: "Cancel" }).click();
  await expect(confirm).toHaveCount(0);
});

test("browser preview refuses to mutate and says why", async ({ page }) => {
  await page.getByRole("button", { name: "Draft" }).first().click();
  await page.getByRole("button", { name: "Confirm" }).click();

  // The preview API rejects writes; that must surface, not fail silently.
  await expect(page.getByRole("alert")).toContainText(/read-only/i);
  await expect(page.getByRole("button", { name: "Confirm" })).toHaveCount(0);
});

test("the Ask Claude panel opens, takes focus, and closes", async ({ page }) => {
  await page.getByRole("button", { name: "Ask Claude" }).click();

  const panel = page.getByRole("complementary", { name: /Ask Claude about this draft/ });
  await expect(panel).toBeVisible();
  await expect(page.getByLabel("Your question")).toBeFocused();

  await page.getByRole("button", { name: "Close chat" }).click();
  await expect(panel).toHaveCount(0);
});

test("a chat failure is reported in the panel", async ({ page }) => {
  await page.getByRole("button", { name: "Ask Claude" }).click();
  await page.getByRole("button", { name: "Who should I take next?" }).click();

  // Browser preview cannot reach the CLI — the panel itself must say so
  // (the header toast about read-only preview is a separate alert).
  const panel = page.getByRole("complementary", { name: /Ask Claude about this draft/ });
  await expect(panel.getByRole("alert")).toContainText(/desktop app/i);
});

test("Escape cancels the draft confirmation and focus returns to the row", async ({ page }) => {
  const draft = page.getByRole("button", { name: "Draft", exact: true }).first();
  await draft.click();

  const dialog = page.getByRole("dialog");
  await expect(dialog).toBeVisible();
  // Focus moves into the dialog, onto the primary action.
  await expect(dialog.getByRole("button", { name: "Confirm" })).toBeFocused();

  await page.keyboard.press("Escape");
  await expect(dialog).toHaveCount(0);
  await expect(draft).toBeFocused();
});

test("column headers sort the board and a second click flips it", async ({ page }) => {
  const adpColumn = () => page.locator("tbody tr td:nth-child(9)").allTextContents();
  const numbers = (cells: string[]) => cells.map(Number).filter((n) => !Number.isNaN(n));

  await page.getByRole("button", { name: "ADP" }).click();
  await expect(page.getByRole("columnheader", { name: "ADP" })).toHaveAttribute(
    "aria-sort",
    "ascending",
  );
  const ascending = numbers(await adpColumn());
  expect(ascending.length).toBeGreaterThan(10);
  expect(ascending).toEqual([...ascending].sort((a, b) => a - b));

  await page.getByRole("button", { name: "ADP" }).click();
  await expect(page.getByRole("columnheader", { name: "ADP" })).toHaveAttribute(
    "aria-sort",
    "descending",
  );
  const descending = numbers(await adpColumn());
  expect(descending).toEqual([...descending].sort((a, b) => b - a));

  // The rank column restores the default order (ranks count drafted players
  // too, so they start above 1 mid-draft — but they climb).
  await page.getByRole("button", { name: "#" }).click();
  const ranks = numbers(await page.locator("tbody tr td:nth-child(1)").allTextContents());
  expect(ranks).toEqual([...ranks].sort((a, b) => a - b));
  expect(ranks[0]).toBeLessThan(ranks[ranks.length - 1]);
});

test("the preview says it is read-only without raising an alarm", async ({ page }) => {
  await expect(page.getByRole("note")).toContainText(/Browser preview/);
  await expect(page.getByRole("alert")).toHaveCount(0);
});

test("the Ask Claude panel sits beside the page instead of over it", async ({ page }) => {
  await page.getByRole("button", { name: "Ask Claude" }).click();
  const panel = page.getByRole("complementary", { name: /Ask Claude about this draft/ });
  await expect(panel).toBeVisible();

  const panelBox = await panel.boundingBox();
  const refresh = await page.getByRole("button", { name: "Refresh data" }).boundingBox();
  const draft = await page.getByRole("button", { name: "Draft" }).first().boundingBox();
  expect(panelBox).not.toBeNull();
  // Nothing the user acts on is underneath the panel.
  expect(refresh!.x + refresh!.width).toBeLessThanOrEqual(panelBox!.x + 1);
  expect(draft!.x + draft!.width).toBeLessThanOrEqual(panelBox!.x + 1);
});

// Dogfood ISSUE-007: the chat took a fixed column and the board was not
// re-flowed, so on any laptop screen the table was cut off — at 1440 the
// Draft button itself was unreachable without scrolling the table sideways.
test.describe("board stays usable with the chat open", () => {
  for (const width of [1440, 1280, 1024]) {
    test(`no clipped columns at ${width}px`, async ({ page }) => {
      await page.setViewportSize({ width, height: 900 });
      await page.getByRole("button", { name: "Ask Claude" }).click();
      await expect(page.locator(".chat")).toBeVisible();

      const clipped = await page.evaluate(() => {
        const wrap = document.querySelector(".board");
        const table = wrap?.querySelector("table");
        if (!wrap || !table) return null;
        return {
          overflow: Math.round(table.getBoundingClientRect().width - wrap.getBoundingClientRect().width),
          wrapRight: Math.round(wrap.getBoundingClientRect().right),
          viewport: window.innerWidth,
        };
      });
      expect(clipped, "board measurements").not.toBeNull();
      expect(clipped!.overflow, "table wider than its column").toBeLessThanOrEqual(1);
      expect(clipped!.wrapRight).toBeLessThanOrEqual(clipped!.viewport + 1);

      // The action that matters most must be on screen, not off the right edge.
      const draft = page.getByRole("button", { name: "Draft" }).first();
      await expect(draft).toBeInViewport();
    });
  }
});
