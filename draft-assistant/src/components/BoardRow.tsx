// One row of the player board, and the shading its survival cell uses.

import { memo } from "react";
import type { AvailablePlayer } from "../types";
import { fmt, injuryTag, pct } from "../format";
import { PlayerName } from "./bits";
import { SecondOpinionCell } from "./SecondOpinion";

/** One board row.
 *
 * Memoised because the whole board re-renders on every 3-second poll, and
 * each row carries two `PlayerName`s whose headshots subscribe to the avatar
 * store — 200 rows meant ~400 subscribers churning per tick for a list whose
 * contents usually did not change.
 */
export const BoardRow = memo(function BoardRow({
  player: p,
  showSecondOpinion,
  onDraft,
}: {
  player: AvailablePlayer;
  showSecondOpinion: boolean;
  onDraft: (id: string, name: string) => void;
}) {
  return (
    <div className={`board-row board-body${showSecondOpinion ? " has-second" : ""}`}>
      <span className="muted right">{p.overall_rank}</span>
      <span className="board-player">
        <PlayerName
          name={p.name}
          team={p.team}
          tag={injuryTag(p.injury_status)}
          playerId={p.player_id}
        />
      </span>
      <span className={`pos-badge pos-${p.position}`}>
        <span>{p.position}</span>
        <span className="pos-rank">{p.position_rank}</span>
      </span>
      {showSecondOpinion && <SecondOpinionCell player={p} />}
      <span className="mid board-team">
        <PlayerName name={p.team ?? "–"} team={p.team} />
      </span>
      <span className="mid right">{p.bye_week ?? "–"}</span>
      <span className="strong right">{fmt(p.points)}</span>
      <span className="mid right">{fmt(p.vorp)}</span>
      <span className={`right tier tier-${Math.min(p.tier, 3)}`}>T{p.tier}</span>
      <span className="mid right">{fmt(p.adp)}</span>
      <span className={`right ${survClass(p.survival_next)}`}>{pct(p.survival_next)}</span>
      <span className="right">
        <button
          type="button"
          className="btn-ghost btn-row"
          onClick={() => onDraft(p.player_id, p.name)}
        >
          Draft
        </button>
      </span>
    </div>
  );
});

function survClass(p: number | null): string {
  if (p === null) return "muted";
  if (p <= 0.25) return "surv-low";
  if (p >= 0.75) return "surv-high";
  return "mid";
}
