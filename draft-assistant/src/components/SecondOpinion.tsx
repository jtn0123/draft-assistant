// One cell of the board's imported-second-opinion column. The rules it reads
// by live in `src/secondOpinion.ts` — see the note there.

import type { AvailablePlayer } from "../types";
import { DISAGREEMENT } from "../secondOpinion";

/** One cell. Tinted, and given a sentence naming the direction, whenever the
 *  two boards are at least `DISAGREEMENT` spots apart. */
export function SecondOpinionCell({ player: p }: { player: AvailablePlayer }) {
  const opinion = p.second_opinion;
  if (opinion === null) return <span className="mid right">–</span>;
  const label = `${p.position}${opinion.positional_rank}`;
  const gap = p.position_rank - opinion.positional_rank;
  if (Math.abs(gap) < DISAGREEMENT) {
    return <span className="mid right">{label}</span>;
  }
  // Positive gap: the source is higher on him than this board is.
  const direction = gap > 0 ? "so-higher" : "so-lower";
  const title = `${opinion.source} has him ${label}; this board has him ${p.position}${p.position_rank}`;
  return (
    <span className={`right so-cell ${direction}`} title={title}>
      {label}
    </span>
  );
}
