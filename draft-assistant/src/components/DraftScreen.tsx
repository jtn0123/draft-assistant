// The draft cockpit: clock, pick queue, three recommendations, rail, board.

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
  // twice reads as a bug even though the underlying scores differ.
  const recommendations = view.recommendations.filter(
    (r, i, all) => i === all.findIndex((x) => x.player_id === r.player_id),
  );
  const rankOf = (playerId: string) =>
    view.available.find((p) => p.player_id === playerId)?.position_rank ?? null;

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
          players={view.available}
          positions={view.league.draftable_positions}
          loading={busy && view.available.length === 0}
          boardSize={view.data_health.board_size}
          onDraft={onDraft}
        />
      </div>
    </div>
  );
}
