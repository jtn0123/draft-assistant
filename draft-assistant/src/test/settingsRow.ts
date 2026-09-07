// Finding a settings-menu row by its label, whichever kind of menu item it is.
//
// A toggle (Pick chime, Live sync) is a menuitemcheckbox and an action
// (Refresh data, Yahoo, Export state) a plain menuitem, so a test that names a
// row by label should not have to know which. A picker's choices are
// menuitemradio items and are looked up by their own names.

import { screen } from "@testing-library/react";

const ROW_ROLES = ["menuitem", "menuitemcheckbox"] as const;

function matches(label: RegExp): HTMLElement[] {
  return ROW_ROLES.flatMap((role) => screen.queryAllByRole(role, { name: label }));
}

/** The row, or null when the menu is closed or has no such row. */
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
