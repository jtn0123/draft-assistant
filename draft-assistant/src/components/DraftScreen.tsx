// The draft cockpit: clock, pick queue, three recommendations, rail, board.

import { useMemo } from "react";
import type { DraftView } from "../types";
import { Board } from "./Board";
import { ClockBanner, SnakeStrip } from "./ClockBanner";
import { RecCard, SidePanel } from "./Panels";

// Ships with this chunk, not with the window.
import "../board.css";

/** The design gives the middle card the emphasised treatment. */
const FEATURED_MODE = "balanced";

export function DraftScreen({
  view,
  busy,
  onDraft,
}: {
  view: DraftView;
  busy: boolean;
  onDraft: (id: string, name: string) => void;
}) {
  // The engine can surface the same player under two modes; showing one card
  // twice reads as a bug even though the underlying scores differ. A seen-set
  // rather than `findIndex`, which made this quadratic.
  const recommendations = useMemo(() => {
    const seen = new Set<string>();
    return view.recommendations.filter((r) => {
      if (seen.has(r.player_id)) return false;
      seen.add(r.player_id);
      return true;
    });
  }, [view.recommendations]);

  // One pass over the pool instead of a full scan per card, and it survives
  // every update that leaves the pool alone (boardIdentity.ts).
  const ranks = useMemo(
    () => new Map(view.available.map((p) => [p.player_id, p.position_rank])),
    [view.available],
  );
  const rankOf = (playerId: string) => ranks.get(playerId) ?? null;

  return (
    <div className="draft-screen">
      <ClockBanner view={view} />
      <SnakeStrip view={view} />

      {view.data_health.warnings.length > 0 && (
        <div className="warnings">{view.data_health.warnings.join(" · ")}</div>
      )}

      {recommendations.length > 0 && (
        <div className="recs">
          {recommendations.map((rec) => (
            <RecCard
              key={rec.mode}
              rec={rec}
              featured={rec.mode === FEATURED_MODE}
              positionRank={rankOf(rec.player_id)}
              onDraft={onDraft}
            />
          ))}
        </div>
      )}

      <div className="draft-body">
        <SidePanel view={view} />
        <Board
          // Keyed by the league, so a switch gets a board with no filters on
          // it. A `DEF` tab pressed in one league is not a filter in a league
          // that has no defences — it is an empty board with no tab lit.
          key={view.league.league_id}
          players={view.available}
          positions={view.league.draftable_positions}
          loading={busy && view.available.length === 0}
          boardSize={view.data_health.board_size}
          secondOpinionLoadedAt={view.data_health.second_opinion_loaded_at}
          onDraft={onDraft}
        />
      </div>
    </div>
  );
}
