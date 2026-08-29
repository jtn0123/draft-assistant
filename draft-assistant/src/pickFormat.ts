import { createContext, useContext } from "react";

/**
 * How to name a pick. "overall" is Sleeper's own numbering — pick 55 of 210.
 * "round" is how drafters talk: round 4, thirteenth pick of it, so 4.13.
 */
export type PickStyle = "overall" | "round";

const KEY = "draft-assistant.pick-style";

/** Overall by default: it is what the Sleeper app and the API both show, so a
 *  first-run number here always matches the number over there. */
export function loadPickStyle(): PickStyle {
  try {
    return window.localStorage.getItem(KEY) === "round" ? "round" : "overall";
  } catch {
    return "overall";
  }
}

export function savePickStyle(style: PickStyle): void {
  try {
    window.localStorage.setItem(KEY, style);
  } catch {
    // Storage unavailable; the choice still applies for this session.
  }
}

/**
 * Round and position within that round, counted the way the board reads: pick
 * 55 of a 14-team draft is the thirteenth pick of round 4, so "4.13". Not the
 * drafting slot — in an even round the snake runs backwards and slot 2 makes
 * that same pick, but nobody calls it 4.2.
 */
export function roundDot(pick: number, teams: number): string {
  if (!Number.isFinite(pick) || teams < 1) return String(pick);
  const round = Math.floor((pick - 1) / teams) + 1;
  const inRound = ((pick - 1) % teams) + 1;
  return `${round}.${inRound}`;
}

export function formatPick(pick: number, teams: number, style: PickStyle): string {
  return style === "round" ? roundDot(pick, teams) : String(pick);
}

/** One choice for the whole app, so the strip, the clock and the chat never
 *  disagree about what to call the pick you are looking at. */
export const PickStyleContext = createContext<PickStyle>("overall");

/** A formatter already bound to this draft's shape and the chosen style. */
export function usePickLabel(teams: number): (pick: number) => string {
  const style = useContext(PickStyleContext);
  return (pick: number) => formatPick(pick, teams, style);
}
