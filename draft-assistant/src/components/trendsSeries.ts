// The arithmetic behind the Trends tab, with no React in it: the range a set
// of readings covers, when they were taken, and whether anything has actually
// moved.
//
// Its own file because both halves of the tab need the same answers — the
// chart scales its axes off them, and the tab decides from them whether a line
// chart is worth drawing at all — and neither should be the other's dependency.

import type { TeamSeries } from "../season-types";

/**
 * The smallest and largest of a list of numbers.
 *
 * `Math.min(...values)` reads better and is a latent crash: it spreads every
 * point of every series into one argument list, and a league with enough
 * snapshots behind it eventually crosses the engine's argument limit and
 * throws `RangeError: too many arguments`. A fold has no such ceiling.
 */
export function extent(values: number[]): { lo: number; hi: number } {
  let lo = Infinity;
  let hi = -Infinity;
  for (const v of values) {
    if (v < lo) lo = v;
    if (v > hi) hi = v;
  }
  return { lo, hi };
}

/** The distinct snapshot times across every series, oldest first. */
export function timeline(series: TeamSeries[]): number[] {
  const seen = new Set<number>();
  for (const s of series) for (const p of s.points) seen.add(p.at);
  return [...seen].sort((a, b) => a - b);
}

/** True once at least one team's line actually goes somewhere. Two readings
 * of identical numbers plot as fourteen flat rules that read as gridlines. */
export function hasMovement(series: TeamSeries[]): boolean {
  return series.some((s) => {
    if (s.points.length <= 1) return false;
    const { lo, hi } = extent(s.points.map((p) => p.strength));
    return hi - lo >= 0.05;
  });
}
