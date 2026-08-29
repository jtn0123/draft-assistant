import type { DraftView } from "../types";
import { Board } from "./Board";
import { ClockBanner, RecCard, SidePanel } from "./Panels";
import { SnakeStrip } from "./SnakeStrip";

/** The draft cockpit: the clock, the snake, the recommendations, the board. */
export function DraftScreen({
  view,
  onDraft,
}: {
  view: DraftView;
  onDraft: (playerId: string, name: string) => void;
}) {
  return (
    <>
      <ClockBanner view={view} />
      <SnakeStrip view={view} />

      {view.data_health.warnings.length > 0 && (
        <div className="warnings">{view.data_health.warnings.join(" · ")}</div>
      )}

      <div className="recs">
        {view.recommendations
          .filter((r, i, all) => i === all.findIndex((x) => x.player_id === r.player_id))
          .map((r) => (
            <RecCard key={r.mode} rec={r} onDraft={onDraft} />
          ))}
      </div>

      <main>
        <SidePanel view={view} />
        <Board
          players={view.available}
          positions={view.league.draftable_positions}
          onDraft={onDraft}
          draftOver={view.draft.status === "complete"}
        />
      </main>
    </>
  );
}
