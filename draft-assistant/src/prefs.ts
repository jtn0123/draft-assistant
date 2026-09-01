// Small preferences that several components read but nothing owns: kept in
// module stores, like the avatar mode, so they do not have to be threaded
// through App.tsx and down as props. The read/guard/write of each one lives
// in persisted.ts; the keys and the stored words are unchanged, so upgrading
// keeps whatever the user had chosen.

import { persisted, usePersisted } from "./persisted";

/** Which of the two main screens the window is showing. */
export type Screen = "draft" | "season";
/** How the head-to-head lineup is laid out. */
export type LineupView = "Table" | "Scoreboard";

const chime = persisted<"on" | "off">("da.chime", (raw) => (raw === "off" ? "off" : "on"), "on");

// Season is the everyday screen; the draft is a few hours a year. The last
// choice is remembered so a draft-night user lands back on the board.
const screen = persisted<Screen>(
  "da.screen",
  (raw) => (raw === "draft" ? "draft" : "season"),
  "season",
);

const lineupView = persisted<LineupView>(
  "da.lineupView",
  (raw) => (raw === "Scoreboard" ? "Scoreboard" : "Table"),
  "Table",
);

export function setChime(next: boolean): void {
  chime.set(next ? "on" : "off");
}

export function useChime(): boolean {
  return usePersisted(chime) === "on";
}

export function setScreen(next: Screen): void {
  screen.set(next);
}

export function useScreen(): Screen {
  return usePersisted(screen);
}

export function setLineupView(next: LineupView): void {
  lineupView.set(next);
}

export function useLineupView(): LineupView {
  return usePersisted(lineupView);
}

/** Test seam: forget this session's choices and re-read what is stored. */
export function resetPrefs(): void {
  chime.reset();
  screen.reset();
  lineupView.reset();
}
