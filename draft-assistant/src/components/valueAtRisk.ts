import type { DraftView } from "../types";

/** Show at most this many; a longer list stops being a decision aid. */
const SHOWN = 5;

/**
 * What waiting costs you.
 *
 * The board already carries each player's odds of surviving to your next
 * pick. Ranking by those odds alone surfaces whoever goes first, which is
 * rarely anyone you wanted; ranking by value alone surfaces the top of the
 * board, which you can already see. The product — value you lose times the
 * chance you lose it — is the thing you are actually trading when you decide
 * to wait, so that is what this sorts on.
 */
export function riskRanked(view: DraftView) {
  return view.available
    .filter((p) => p.survival_next !== null && p.vorp > 0)
    .map((p) => ({ player: p, cost: p.vorp * (1 - (p.survival_next ?? 1)) }))
    .filter((r) => r.cost > 0)
    .sort((a, b) => b.cost - a.cost)
    .slice(0, SHOWN);
}
