// The Projections tab, in a real browser.
//
// The screen has unit tests over hand-made rows; what those cannot show is
// that the tab exists, that the shell routes to it, that the draft view
// reaches it with `draft_projections` on board, and that a fourteen-team
// table lays out inside the window. The fixture's projections are all zeroes
// with the season's slots still open — the checked-in board carries no
// projection for a drafted player — so what is asserted here is the shape of
// the screen rather than the size of its numbers.

import { expect, test } from "@playwright/test";

test("the projections tab lists every roster in the draft", async ({ page }, info) => {
  await page.goto("/");
  await page.getByRole("button", { name: "Projections", exact: true }).click();

  const rows = page.locator(".proj-draft-row.proj-body");
  await expect(rows).toHaveCount(14);
  // The two headline numbers, and the table's own columns.
  await expect(page.getByText("Your draft", { exact: true })).toBeVisible();
  await expect(page.getByText("Wins the league", { exact: true })).toBeVisible();
  for (const column of ["Starters", "Bench", "Wins it", "Still to fill"]) {
    await expect(page.getByText(column, { exact: true })).toBeVisible();
  }
  // Missing projections must not invent a ranking or a 100% favorite.
  for (const row of await rows.all()) {
    await expect(row.locator("span").first()).toHaveText("-");
    await expect(row).toContainText("Not available");
  }
  await expect(page.getByText("100%", { exact: true })).toHaveCount(0);

  // Exactly one row is yours, and the screen says so twice: in the table and
  // in the sentence under the headline.
  await expect(page.locator(".proj-draft-row.is-mine")).toHaveCount(1);
  await expect(page.getByText("Waiting for projected starters")).toBeVisible();

  // A table this wide is the first thing to overflow a narrow window; the
  // board's own guard covers the board, and this covers the new screen.
  const overflow = await page.evaluate(
    () => document.documentElement.scrollWidth - document.documentElement.clientWidth,
  );
  expect(overflow).toBeLessThanOrEqual(0);
  await page.screenshot({ path: info.outputPath("projections.png") });
});

test("the tab keeps its place across a reload and lets the board back", async ({ page }) => {
  await page.goto("/");
  await page.getByRole("button", { name: "Projections", exact: true }).click();
  await expect(page.locator(".proj-draft-row.proj-body").first()).toBeVisible();

  // The screen is a remembered preference, like Draft and Season.
  await page.reload();
  await expect(page.getByRole("button", { name: "Projections", exact: true })).toHaveAttribute(
    "aria-pressed",
    "true",
  );
  await expect(page.locator(".proj-draft-row.proj-body")).toHaveCount(14);

  await page.getByRole("button", { name: "Draft", exact: true }).click();
  await expect(page.locator(".board")).toBeVisible();
  await expect(page.locator(".proj-draft-row.proj-body")).toHaveCount(0);
});
