// Settings -> "Check for updates": the row's states and the copy for each.
//
// Pure. The row moves through idle, checking, up to date, available,
// installing and failed; what selecting it does depends only on where it is,
// and what it says depends only on where it is and what the check found. The
// hook in useUpdateRow.ts owns the promise and the React state; everything a
// test wants to read is here, with nothing rendered.

import type { SettingsRow } from "./components/Header";

/** What `check_for_update` hands back. `available` is null when the running
 *  copy is the newest release on the feed. */
export interface UpdateCheck {
  current: string;
  available: string | null;
  notes: string | null;
}

export type UpdateState =
  | { kind: "idle" }
  | { kind: "checking" }
  | { kind: "current" }
  | { kind: "available"; version: string; notes: string | null }
  | { kind: "installing"; version: string }
  | { kind: "failed"; message: string };

export const IDLE: UpdateState = { kind: "idle" };

/** What a finished check moves the row to. */
export function settled(check: UpdateCheck): UpdateState {
  if (check.available === null) return { kind: "current" };
  return { kind: "available", version: check.available, notes: check.notes };
}

/** What a failed check or install moves the row to. The message is the
 *  backend's sentence when there is one; anything else gets a plain fallback
 *  rather than "[object Object]" on the menu. */
export function failed(error: unknown): UpdateState {
  const message =
    error instanceof Error
      ? error.message
      : typeof error === "string" && error.trim() !== ""
        ? error
        : "The update check failed. Try again";
  return { kind: "failed", message };
}

/** The first line of the release notes, for the row's one-line note. Headings
 *  are skipped: release.yml's notes open with the version as a heading, and
 *  the row already names the version in its label. Bullet markers are
 *  dropped. Null when nothing but headings and blanks remain. */
export function firstLine(notes: string | null): string | null {
  if (notes === null) return null;
  const line = notes
    .split("\n")
    .map((l) => l.trim())
    .filter((l) => l !== "" && !l.startsWith("#"))
    .map((l) => l.replace(/^[\s*-]+/, "").trim())
    .find((l) => l !== "");
  return line ?? null;
}

/** What selecting the row means in each state: a check, an install, or
 *  nothing while one is already in flight. */
export function selection(state: UpdateState): "check" | "install" | "none" {
  switch (state.kind) {
    case "idle":
    case "current":
    case "failed":
      return "check";
    case "available":
      return "install";
    case "checking":
    case "installing":
      return "none";
  }
}

/** The state the row shows the moment it is selected, before anything
 *  answers. */
export function started(state: UpdateState): UpdateState {
  switch (selection(state)) {
    case "check":
      return { kind: "checking" };
    case "install":
      return { kind: "installing", version: state.kind === "available" ? state.version : "" };
    case "none":
      return state;
  }
}

/** The row itself. `current` is the running version; `onSelect` is what the
 *  shell wired to the row and is called in every state, since the pure
 *  `selection` above is what decides whether it does anything. */
export function updateRow(current: string, state: UpdateState, onSelect: () => void): SettingsRow {
  switch (state.kind) {
    case "idle":
      return {
        label: "Check for updates",
        note: `Ask the release feed for something newer than v${current}`,
        value: "Check",
        on: false,
        onSelect,
      };
    case "checking":
      return {
        label: "Check for updates",
        note: "Asking the release feed…",
        value: "…",
        on: false,
        onSelect,
      };
    case "current":
      return {
        label: "Up to date",
        note: `v${current} is the newest release. Select to check again`,
        value: `v${current}`,
        on: true,
        onSelect,
      };
    case "available":
      return {
        label: `Update to ${state.version}`,
        note: firstLine(state.notes) ?? "Select to download it. The app restarts on its own",
        value: "Install",
        on: true,
        onSelect,
      };
    case "installing":
      return {
        label: `Update to ${state.version}`,
        note: "Downloading and verifying. The app restarts on its own…",
        value: "…",
        on: true,
        onSelect,
      };
    case "failed":
      return {
        label: "Check for updates",
        note: state.message,
        value: "Retry",
        on: false,
        onSelect,
      };
  }
}
