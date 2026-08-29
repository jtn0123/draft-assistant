import type { DraftView } from "../types";
import { fmt, pct } from "../format";
import { riskRanked } from "./valueAtRisk";
import { usePickLabel } from "../pickFormat";

export function AtRisk({ view }: { view: DraftView }) {
  const next = view.draft.my_next_picks.find((p) => p > view.draft.current_pick);
  const rows = riskRanked(view);
  const label = usePickLabel(view.draft.teams);
  if (next === undefined || rows.length === 0) return null;
  return (
    <section>
      <h2>Won&apos;t last to {label(next)}</h2>
      <ul className="at-risk" aria-label="Players unlikely to last">
        {rows.map(({ player }) => (
          <li key={player.player_id}>
            <span className={`pos-badge pos-${player.position}`}>{player.position}</span>
            <span className="at-risk-name">{player.name}</span>
            <span className="muted">{fmt(player.vorp)} VORP</span>
            <span className={survivalClass(player.survival_next)}>
              {pct(player.survival_next)}
            </span>
          </li>
        ))}
      </ul>
      <p className="muted small-text">
        Chance each is still there at your pick {label(next)}, worst value-at-risk first.
      </p>
    </section>
  );
}

function survivalClass(survival: number | null): string {
  if (survival === null) return "muted";
  if (survival <= 0.25) return "surv low";
  if (survival <= 0.6) return "surv mid";
  return "surv";
}
