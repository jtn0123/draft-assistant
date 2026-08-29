import type { DraftView, TeamProjection } from "../types";
import { fmt, pct } from "../format";

/**
 * The draft's scoreboard: every team's best lineup, ranked by what it
 * projects to over the season with byes honoured. Your row is marked. A
 * team with no picks yet projects to nothing and sits at the bottom.
 */
export function Standings({ view }: { view: DraftView }) {
  const rows = view.projected_standings;
  if (rows.length === 0) return null;
  const mine = view.draft.my_slot;
  const top = rows[0]?.season ?? 0;
  const week = rows[0]?.week ?? 1;
  const hasWeek = rows.some((t) => t.week_points > 0);
  const odds = new Map(view.playoff_odds.map((o) => [o.slot, o]));
  const hasOdds = odds.size > 0;
  return (
    <section>
      <h2>Projected standings</h2>
      <ol className="standings" aria-label="Projected standings">
        <li className="standings-head muted" aria-hidden="true">
          <span />
          <span />
          <span className="standings-pts">Season</span>
          <span className="standings-gap">{hasWeek ? `Wk ${week}` : ""}</span>
          {hasOdds && <span className="standings-odds">Playoffs</span>}
        </li>
        {rows.map((t, i) => (
          <li key={t.slot} className={t.slot === mine ? "mine" : ""} title={lineupTitle(t)}>
            <span className="standings-rank">{i + 1}</span>
            <span className="standings-name">
              {t.slot === mine ? "YOU" : (t.display_name ?? `Slot ${t.slot}`)}
            </span>
            <span className="standings-pts">
              {fmt(t.season)}
              {i > 0 && <span className="muted"> (−{fmt(top - t.season)})</span>}
            </span>
            <span className="standings-gap">{hasWeek ? fmt(t.week_points, 1) : ""}</span>
            {hasOdds && (
              <span
                className="standings-odds"
                title={odds.get(t.slot) ? `${fmt(odds.get(t.slot)!.expected_wins, 1)} expected wins over ${odds.get(t.slot)!.runs} simulated seasons` : undefined}
              >
                {odds.get(t.slot) ? pct(odds.get(t.slot)!.playoff_odds) : ""}
              </span>
            )}
          </li>
        ))}
      </ol>
      <p className="muted small-text">
        Season: best lineup each week from the players drafted, byes honoured, under this
        league&apos;s scoring.{hasWeek && ` Wk ${week}: that week's own projections.`}
        {hasOdds && " Playoffs: simulated rest of season on the league's schedule."} Hover a row
        for the lineups.
      </p>
    </section>
  );
}

function lineupTitle(t: TeamProjection): string {
  const line = (xs: TeamProjection["starters"]) =>
    xs.map((s) => `${s.slot} ${s.name} (${fmt(s.points, 1)})`).join("\n");
  const season = `${fmt(t.full_strength)} at full strength\n${line(t.starters)}`;
  if (t.week_starters.length === 0) return season;
  return `${season}\n\nWeek ${t.week}: ${fmt(t.week_points, 1)}\n${line(t.week_starters)}`;
}
