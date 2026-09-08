import { expect, test } from "@playwright/test";

test("settings has a dedicated page and returns to the same draft", async ({ page }, info) => {
  await page.goto("/");
  await page.getByRole("button", { name: "Draft", exact: true }).click();
  const search = page.getByLabel("Search players");
  await search.fill("Josh");
  await page.getByRole("button", { name: "Settings", exact: true }).click();
  const quick = page.getByRole("menu", { name: "Settings" });
  await expect(quick.getByRole("menuitemcheckbox", { name: /Live sync/ })).toBeVisible();
  await expect(quick.getByRole("group", { name: "Appearance" })).toHaveCount(0);
  await quick.getByRole("menuitem", { name: /All settings/ }).click();
  const settings = page.getByRole("dialog", { name: "Settings", exact: true });
  await expect(settings).toBeVisible();
  await expect(quick).toHaveCount(0);
  await expect(settings.getByLabel("Sleeper username", { exact: true })).toBeVisible();
  for (const name of [
    "Draft identity",
    "Remote connections",
    "Appearance & sound",
    "Draft data",
    "Diagnostics & updates",
  ]) {
    await expect(settings.getByRole("heading", { name, exact: true })).toHaveCount(1);
  }
  await settings.getByRole("link", { name: "Appearance & sound" }).click();
  await settings.getByRole("button", { name: "Dark", exact: true }).click();
  await expect(settings.getByRole("button", { name: "Dark", exact: true })).toHaveAttribute(
    "aria-pressed",
    "true",
  );
  await expect(page.locator("html")).toHaveAttribute("data-theme", "dark");
  expect(await settings.evaluate((element) => element.scrollWidth <= element.clientWidth)).toBe(
    true,
  );
  await settings.getByRole("link", { name: "Draft identity", exact: true }).click();
  await page.screenshot({ path: info.outputPath("settings-page.png") });
  await page.keyboard.press("Escape");
  await expect(settings).toHaveCount(0);
  await expect(page.getByRole("button", { name: "Settings", exact: true })).toBeFocused();
  await expect(search).toHaveValue("Josh");
});
