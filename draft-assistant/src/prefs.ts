// Small preferences that several components read but nothing owns: kept in
// module stores, like the avatar mode, so they do not have to be threaded
// through App.tsx and down as props. The read/guard/write of each one lives
// in persisted.ts; the keys and the stored words are unchanged, so upgrading
// keeps whatever the user had chosen.

import { persisted, usePersisted } from "./persisted";

/** Which of the three main screens the window is showing. */
export type Screen = "draft" | "season" | "projections";
/** How the head-to-head lineup is laid out. */
export type LineupView = "Table" | "Scoreboard";

const chime = persisted<"on" | "off">("da.chime", (raw) => (raw === "off" ? "off" : "on"), "on");

// Season is the everyday screen; the draft is a few hours a year. The last
// choice is remembered so a draft-night user lands back on the board.
const screen = persisted<Screen>(
  "da.screen",
  (raw) => (raw === "draft" || raw === "projections" ? raw : "season"),
  "season",
);

// The header's Ask AI button. Off by default: the panel is a few hours a
// year and the button sat in the header the rest of the time, so it is
// switched on from the settings menu by whoever wants it there.
const askButton = persisted<"on" | "off">(
  "da.askButton",
  (raw) => (raw === "on" ? "on" : "off"),
  "off",
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

export function setAskButton(next: boolean): void {
  askButton.set(next ? "on" : "off");
}

export function useAskButton(): boolean {
  return usePersisted(askButton) === "on";
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
  askButton.reset();
  screen.reset();
  lineupView.reset();
}
