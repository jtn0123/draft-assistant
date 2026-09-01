// Keeps `view.available` referentially stable across updates that did not
// change it.
//
// Grade item G7. The board filters and sorts several hundred players in a
// `useMemo` keyed on the array it was handed, and every `DraftView` the
// backend produces carries a brand-new one. So an update that only flipped
// the draft's status, or a "Refresh data" that came back with the same
// numbers, re-ran the whole filter and sort for a board that had not moved.
//
// The fix has to be a cache key that cannot go stale. A cheap signature over
// pick counts and array lengths is not one: `refresh_data` rebuilds the board
// from freshly fetched projections without touching either, so a key that
// coarse would leave the old points on the screen — which is exactly why
// `players` was left in the memo's dependencies in the first place.
//
// So compare the data instead, once per applied view. Every field of every
// player: O(n) primitive comparisons, against the O(n log n) sort it saves.
// Only when the comparison says the two arrays say the same thing do we keep
// the old one, and reusing it is then indistinguishable from using the new
// one — there is no observable difference between them.

import type { AvailablePlayer, DraftView } from "./types";

// Spelled out as an object literal so `satisfies` makes this exhaustive: add a
// field to `AvailablePlayer` and this stops compiling until it is compared
// too. Missing one would be exactly the stale-data bug the cache exists to
// avoid.
const COMPARED = {
  player_id: true,
  name: true,
  position: true,
  team: true,
  bye_week: true,
  points: true,
  bonus_points: true,
  vorp: true,
  tier: true,
  position_rank: true,
  overall_rank: true,
  adp: true,
  injury_status: true,
  sleeper_pts_ppr: true,
  survival_next: true,
} satisfies Record<keyof AvailablePlayer, true>;

const FIELDS = Object.keys(COMPARED) as (keyof AvailablePlayer)[];

function samePlayer(a: AvailablePlayer, b: AvailablePlayer): boolean {
  for (const field of FIELDS) {
    if (a[field] !== b[field]) return false;
  }
  return true;
}

/**
 * Whether two available-player lists carry the same data in the same order.
 * Order counts: the board's sort is only ever stable relative to its input.
 *
 * Only `stableAvailable` below calls this; it is exported so the comparison
 * can be tested field by field, which is the part that has to be exhaustive.
 */
export function sameAvailable(a: AvailablePlayer[], b: AvailablePlayer[]): boolean {
  if (a === b) return true;
  if (a.length !== b.length) return false;
  for (let i = 0; i < a.length; i += 1) {
    if (!samePlayer(a[i], b[i])) return false;
  }
  return true;
}

/**
 * `next`, with the previous `available` array spliced back in when the new one
 * says exactly the same thing. Everything else comes from `next` as usual —
 * only the array identity is recycled.
 */
export function stableAvailable(prev: DraftView | null, next: DraftView): DraftView {
  if (prev === null || !sameAvailable(prev.available, next.available)) return next;
  return { ...next, available: prev.available };
}
