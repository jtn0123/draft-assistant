// The rules the board's imported-second-opinion column reads by: whether
// there is a column at all, what heads it, and how it sorts.
//
// Split from the cell component next door only because a file that exports
// both a component and its helpers loses fast refresh. The division of labour
// that matters is the other one: the backend decides *what* a second opinion
// is, this decides how a disagreement reads.

import type { AvailablePlayer } from "./types";

/** How far apart the two positional ranks must be to be worth pointing at.
 *  Matches `DISAGREEMENT` in `src-tauri/src/second_opinion.rs`. */
export const DISAGREEMENT = 8;

/** True when any player on the board carries an imported opinion. Nothing
 *  imported means no column at all, rather than a column of dashes. */
export function hasSecondOpinion(players: AvailablePlayer[]): boolean {
  return players.some((p) => p.second_opinion !== null);
}

/** The source label to head the column with — "Clay", "FantasyPros". */
export function secondOpinionSource(players: AvailablePlayer[]): string {
  return players.find((p) => p.second_opinion !== null)?.second_opinion?.source ?? "Imported";
}

/** The column header's tooltip: who said it, and when it was imported. */
export function secondOpinionTitle(source: string, loadedAt: number | null): string {
  const when =
    loadedAt === null || loadedAt === 0
      ? "imported"
      : `imported ${new Date(loadedAt * 1000).toLocaleDateString()}`;
  return `${source}'s rank at the position, ${when}. Ranks compare across scoring systems; points do not.`;
}

/** Sort key for the column: players with nothing imported sort last, which is
 *  what `compare` in Board.tsx does with a null. */
export function secondOpinionRank(player: AvailablePlayer): number | null {
  return player.second_opinion?.positional_rank ?? null;
}
