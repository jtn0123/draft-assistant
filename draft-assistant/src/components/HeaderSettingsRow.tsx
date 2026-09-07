// One row of the settings menu, and the shape of what the shell hands it.
//
// Split from Header.tsx when the rows stopped being one kind of thing. A row
// is an action (Refresh data, Sign out of Yahoo, Check for updates), a toggle
// with a state of its own (Pick chime, Live sync), or a picker with a few
// choices of which one is current (Appearance). Screen readers name the three
// differently, and a menu that called every one of them a checkbox told the
// listener "Check for updates, not checked", which is not a thing.

import type { KeyboardEvent, Ref } from "react";

export type SettingsRowKind = "action" | "toggle" | "radio";

/** One of a radio row's choices. */
export interface SettingsOption {
  id: string;
  label: string;
  on: boolean;
  onSelect: () => void;
}

export interface SettingsRow {
  /** Stable while the row is on the menu, whatever its label does: React's
   *  key. Keyed by label, the updater row was a new element every time it
   *  went from "Check for updates" to "Checking…" to "Update to 0.3.2", and
   *  the keyboard fell off it at each step. */
  id: string;
  kind: SettingsRowKind;
  label: string;
  note: string;
  value: string;
  /** Lights the value up. For a toggle it is also the state a screen reader
   *  is told; an action's `on` is only the colour. */
  on: boolean;
  onSelect: () => void;
  /** A radio row's choices, exactly one of them `on`. Ignored elsewhere. */
  options?: SettingsOption[];
}

/** The selector for everything the arrow keys walk. */
export const MENU_ITEMS = '[role="menuitem"], [role="menuitemcheckbox"], [role="menuitemradio"]';

export function SettingsMenuRow({
  row,
  firstRef,
  onKeyDown,
}: {
  row: SettingsRow;
  /** Set on the row that takes focus when the menu opens. */
  firstRef?: Ref<HTMLButtonElement>;
  onKeyDown: (event: KeyboardEvent) => void;
}) {
  const text = (
    <span className="settings-row-text">
      <span className="settings-row-label">{row.label}</span>
      <span className="muted settings-row-note">{row.note}</span>
    </span>
  );

  if (row.kind === "radio") {
    const options = row.options ?? [];
    return (
      <div className="settings-row settings-row-radio" role="none">
        {text}
        <span className="settings-radio" role="group" aria-label={row.label}>
          {options.map((option, index) => (
            <button
              key={option.id}
              type="button"
              className={option.on ? "settings-radio-option is-on" : "settings-radio-option"}
              role="menuitemradio"
              aria-checked={option.on}
              tabIndex={-1}
              ref={index === 0 ? firstRef : undefined}
              onClick={option.onSelect}
              onKeyDown={onKeyDown}
            >
              {option.label}
            </button>
          ))}
        </span>
      </div>
    );
  }

  // A toggle's setting is its state, not a word in its label: a screen reader
  // should say "on", not read "On" as part of the name and leave the listener
  // to guess it was a control. An action has no state to report.
  const toggle = row.kind === "toggle";
  return (
    <button
      type="button"
      className="settings-row"
      role={toggle ? "menuitemcheckbox" : "menuitem"}
      aria-checked={toggle ? row.on : undefined}
      tabIndex={-1}
      ref={firstRef}
      onClick={row.onSelect}
      onKeyDown={onKeyDown}
    >
      {text}
      <span className={row.on ? "settings-row-value is-on" : "settings-row-value"}>
        {row.value}
      </span>
    </button>
  );
}
