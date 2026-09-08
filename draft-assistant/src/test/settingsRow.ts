// Settings rows may live in the quick menu or the full settings dialog.
// Keep the menu roles for Header unit tests; the page uses buttons and switches.

import { screen, within } from "@testing-library/react";

import { settle } from "./settle";

/** Open the dedicated settings page through its quick-menu entry. */
export async function openSettingsPage(): Promise<void> {
  if (screen.queryByRole("dialog", { name: "Settings" })) return;
  await settle(() => {
    if (!screen.queryByRole("menu")) screen.getByRole("button", { name: "Settings" }).click();
  });
  await settle(() => screen.getByRole("menuitem", { name: /All settings/ }).click());
  await screen.findByRole("dialog", { name: "Settings" });
}

const ROW_ROLES = ["menuitem", "menuitemcheckbox"] as const;

function matches(label: RegExp): HTMLElement[] {
  const page = screen.queryByRole("dialog", { name: "Settings" });
  if (page)
    return ["button", "switch"]
      .flatMap((role) => within(page).queryAllByRole(role, { name: label }))
      .filter((row) => row.classList.contains("settings-page-row"));
  return ROW_ROLES.flatMap((role) => screen.queryAllByRole(role, { name: label }));
}

/** The row, or null when neither settings surface offers it. */
export function querySettingsRow(label: RegExp): HTMLElement | null {
  const found = matches(label);
  if (found.length > 1) throw new Error(`${found.length} settings rows match ${String(label)}`);
  return found[0] ?? null;
}

/** The row; throws when there is not exactly one. */
export function settingsRow(label: RegExp): HTMLElement {
  const found = querySettingsRow(label);
  if (found === null) throw new Error(`No settings row matches ${String(label)}`);
  return found;
}
