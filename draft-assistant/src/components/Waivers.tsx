import type { DraftView } from "../types";
import { fmt } from "../format";
import { PlayerName } from "./PlayerCard";

/**
 * The waiver wire, priced for this roster. Gain is what a player adds to
 * the season lineup total with byes honoured — a receiver behind seven of
 * your own scores zero here however good he is. Rivals is how many other
 * teams he would also lift: the competition for the claim.
 */
export function Waivers({ view }: { view: DraftView }) {
  const w = view.waivers;
  if (!w || w.targets.length === 0) return null;
  const worth = w.targets.filter((t) => t.my_gain >= 1);
  return (
    <section className="waivers">
      <h2>Waiver targets</h2>
      {worth.length === 0 ? (
        <p className="muted">Nobody on the wire would start for you.</p>
      ) : (
        <ul className="waiver-list" aria-label="Waiver targets">
          {worth.map((t) => (
            <li key={t.player_id} title={`${fmt(t.points)} season pts · bye ${t.bye_week ?? "–"}`}>
              <span className={`pos-badge pos-${t.position}`}>{t.position}</span>
              <span className="waiver-name">
                <PlayerName id={t.player_id}>{t.name}</PlayerName>
                {t.team && <span className="muted"> {t.team}</span>}
                {t.bye_week != null && <span className="muted"> · bye {t.bye_week}</span>}
              </span>
              <span className="gain">
                +{fmt(t.my_gain)}
                {t.suggested_bid != null && <span className="muted"> · ${t.suggested_bid}</span>}
              </span>
              <span className="muted waiver-demand">
                {t.rivals_helped > 0 && `${t.rivals_helped} rival${t.rivals_helped === 1 ? "" : "s"}`}
                {t.trending_adds != null && ` · 🔥${compact(t.trending_adds)}`}
              </span>
            </li>
          ))}
        </ul>
      )}
      {w.drops.length > 0 && (
        <p className="muted small-text">
          Drop first:{" "}
          {w.drops
            .filter((d) => d.starts === 0)
            .map((d) => `${d.name} (${d.position}, never starts)`)
            .join(", ") || "everyone starts at least once"}
        </p>
      )}
    </section>
  );
}

function compact(n: number): string {
  return n >= 1000 ? `${Math.round(n / 1000)}k` : String(n);
}
