// The board's columns: what each one is called, what it reads off a player,
// and how two of those values are ordered.
//
// Lifted out of Board.tsx, which was closing on the 500-line cap. Keeping the
// table of columns apart from the component that draws it also means the
// ordering rules can be read without wading through JSX.

import type { AvailablePlayer } from "../types";
import {
  hasSecondOpinion,
  secondOpinionRank,
  secondOpinionSource,
  secondOpinionTitle,
} from "../secondOpinion";

export type SortKey =
  "rank" | "name" | "pos" | "second" | "team" | "bye" | "pts" | "vorp" | "tier" | "adp" | "surv";

export type Direction = "asc" | "desc";

export interface Column {
  key: SortKey;
  label: string;
  right: boolean;
  title?: string;
  /** Natural first direction — points sort high-to-low, names A-to-Z. */
  initial: Direction;
  value: (p: AvailablePlayer) => string | number | null;
}

export const COLUMNS: Column[] = [
  { key: "rank", label: "#", right: true, initial: "asc", value: (p) => p.overall_rank },
  { key: "name", label: "Player", right: false, initial: "asc", value: (p) => p.name },
  { key: "pos", label: "Pos", right: false, initial: "asc", value: (p) => p.position },
  { key: "team", label: "Team", right: false, initial: "asc", value: (p) => p.team },
  { key: "bye", label: "Bye", right: true, initial: "asc", value: (p) => p.bye_week },
  {
    key: "pts",
    label: "Pts",
    right: true,
    initial: "desc",
    title: "Season points under your league's exact scoring",
    value: (p) => p.points,
  },
  {
    key: "vorp",
    label: "Vorp",
    right: true,
    initial: "desc",
    title: "Value over replacement",
    value: (p) => p.vorp,
  },
  { key: "tier", label: "Tier", right: true, initial: "asc", value: (p) => p.tier },
  { key: "adp", label: "Adp", right: true, initial: "asc", value: (p) => p.adp },
  {
    key: "surv",
    label: "Surv",
    right: true,
    initial: "asc",
    title: "Chance they survive to your next pick",
    value: (p) => p.survival_next,
  },
];

/** The imported column, built only when there is something to put in it. It
 *  sits immediately after this board's own positional rank, which is the
 *  number it is there to be compared against. */
export function columnsFor(players: AvailablePlayer[], loadedAt: number | null): Column[] {
  if (!hasSecondOpinion(players)) return COLUMNS;
  const source = secondOpinionSource(players);
  const at = COLUMNS.findIndex((c) => c.key === "pos") + 1;
  const column: Column = {
    key: "second",
    label: source,
    right: true,
    initial: "asc",
    title: secondOpinionTitle(source, loadedAt),
    value: secondOpinionRank,
  };
  return [...COLUMNS.slice(0, at), column, ...COLUMNS.slice(at)];
}

export const SORT_LABEL: Record<SortKey, string> = {
  rank: "rank",
  name: "name",
  pos: "position",
  second: "the imported rank",
  team: "team",
  bye: "bye week",
  pts: "points",
  vorp: "VORP",
  tier: "tier",
  adp: "ADP",
  surv: "survival",
};

/** Order two cells, blanks last whichever way the column points.
 *
 * The direction is applied in here rather than by the caller: a blank that
 * answered "after you" would have become "before you" the moment the sort was
 * flipped, floating every free agent to the top of a descending Team sort.
 */
export function compare(
  a: string | number | null,
  b: string | number | null,
  sign: number,
): number {
  if (a === null && b === null) return 0;
  if (a === null) return 1;
  if (b === null) return -1;
  if (typeof a === "string" && typeof b === "string") return a.localeCompare(b) * sign;
  return (Number(a) - Number(b)) * sign;
}
