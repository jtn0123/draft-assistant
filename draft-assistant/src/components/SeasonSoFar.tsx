import type { DraftView } from "../types";
import { fmt, ordinal } from "../format";

/**
 * The season so far: my record and place, each week's result, and who on
 * my roster is beating his projection. Trends only mean something after a
 * few weeks; before then they are shown but not shouted.
 */
export function SeasonSoFar({ view }: { view: DraftView }) {
  const s = view.season;
  if (!s) return null;
  const mine = view.draft.my_slot;
  const me = s.standings.find((r) => r.slot === mine);
  const place = me ? s.standings.indexOf(me) + 1 : null;
  const up = s.trends.filter((t) => t.delta_per_game > 0).slice(0, 3);
  const down = s.trends.filter((t) => t.delta_per_game < 0).slice(-3).reverse();
  return (
    <section className="season">
      <h2>Season through week {s.through_week}</h2>
      {me && (
        <p className="strong">
          {me.wins}–{me.losses}
          {me.ties > 0 && `–${me.ties}`} · {ordinal(place ?? 0)} of {s.standings.length} ·{" "}
          {fmt(me.points_for)} for
        </p>
      )}
      {s.my_results.length > 0 && (
        <ol className="results" aria-label="My results">
          {s.my_results.map((r) => (
            <li key={r.week} className={r.won === null ? "" : r.won ? "won" : "lost"}>
              <span className="muted">Wk {r.week}</span>
              <span>{r.won === null ? "—" : r.won ? "W" : "L"}</span>
              <span className="standings-pts">
                {fmt(r.my_points, 1)}
                {r.opponent_points !== null && ` – ${fmt(r.opponent_points, 1)}`}
              </span>
              <span className="muted standings-name">{r.opponent_name ?? ""}</span>
            </li>
          ))}
        </ol>
      )}
      {s.trends.length > 0 && (
        <p className="muted small-text">
          {up.length > 0 && `Beating projection: ${up.map(trend).join(", ")}. `}
          {down.length > 0 && `Behind it: ${down.map(trend).join(", ")}.`}
        </p>
      )}
    </section>
  );
}

function trend(t: { name: string; delta_per_game: number; games: number }): string {
  const sign = t.delta_per_game > 0 ? "+" : "";
  return `${t.name} ${sign}${fmt(t.delta_per_game, 1)}/g over ${t.games}`;
}
