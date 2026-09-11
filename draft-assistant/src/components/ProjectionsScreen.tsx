// The projections screen: everything the season model expects from here, in
// one place.
//
// The numbers themselves are not new — `season_odds.rs` runs the same 4,000
// simulations over the league's real remaining schedule for the Season
// screen's standings and header. What was missing was somewhere to read them
// as a forecast rather than as a column beside a record: the league ordered by
// what it projects from here, next to what it has actually banked, with this
// week's matchup priced at the top.

import type { SeasonView, StandingsRow } from "../season-types";
import type { DraftView, TeamProjection } from "../types";
import { fmt, pct } from "../format";
import { ODDS_NOTE } from "../odds";

import "../board.css";
import "../projections.css";

/** A team's rank by what it projects, against where it sits today. */
function Move({ seed, rank }: { seed: number; rank: number }) {
  const move = seed - rank;
  if (move === 0) return <span className="muted">·</span>;
  return (
    <span className={move > 0 ? "proj-move is-up" : "proj-move is-down"}>
      {move > 0 ? `+${move}` : move}
    </span>
  );
}

function Week({ view }: { view: SeasonView }) {
  const { header } = view;
  if (header.opponent_name === null) return null;
  const margin = header.my_projected - header.opp_projected;
  return (
    <section className="proj-card proj-week">
      <div className="proj-week-head">
        <span className="eyebrow">Week {view.week}</span>
        <span className="muted">{ODDS_NOTE}</span>
      </div>
      <div className="proj-week-body">
        <div className="proj-side">
          <span className="muted">You</span>
          <strong>{fmt(header.my_projected, 1)}</strong>
        </div>
        <div className="proj-margin">
          <span className={margin >= 0 ? "proj-move is-up" : "proj-move is-down"}>
            {margin >= 0 ? `+${fmt(margin, 1)}` : fmt(margin, 1)}
          </span>
          <span className="muted">{pct(header.win_odds_best)} to win</span>
        </div>
        <div className="proj-side is-them">
          <span className="muted">{header.opponent_name}</span>
          <strong>{fmt(header.opp_projected, 1)}</strong>
        </div>
      </div>
      {header.my_set_projected < header.my_projected && (
        <p className="muted small">
          Your set lineup projects {fmt(header.my_set_projected, 1)} and wins{" "}
          {pct(header.win_odds_set)} of the time:{" "}
          {fmt(header.my_projected - header.my_set_projected, 1)} points are on your bench.
        </p>
      )}
    </section>
  );
}

function Table({ rows }: { rows: StandingsRow[] }) {
  // Ordered by what each roster projects from here, which is the whole point
  // of the screen; the seed it holds today rides along so the gap between the
  // two is visible.
  const ordered = [...rows].sort((a, b) => b.projected_points - a.projected_points);
  return (
    <section className="proj-card proj-table">
      <div className="proj-row proj-head">
        <span>#</span>
        <span>Team</span>
        <span className="right">W-L</span>
        <span className="right">Scored</span>
        <span className="right">Projected</span>
        <span className="right">Playoffs</span>
        <span className="right">Seed</span>
      </div>
      {ordered.map((row, index) => (
        <div
          key={row.roster_id}
          className={row.is_mine ? "proj-row proj-body is-mine" : "proj-row proj-body"}
        >
          <span className="muted num">{index + 1}</span>
          <span className="ellipsis">{row.name}</span>
          <span className="right muted">{row.record}</span>
          <span className="right num">{fmt(row.points_for, 0)}</span>
          <span className="right num proj-points">{fmt(row.projected_points, 0)}</span>
          <span className="right num">{row.playoff_status ?? pct(row.playoff_odds)}</span>
          <span className="right">
            <Move seed={row.seed} rank={index + 1} />
          </span>
        </div>
      ))}
    </section>
  );
}

/** The league as the draft has left it: who drafted the most points. */
function FromTheDraft({ rows }: { rows: TeamProjection[] }) {
  const mine = rows.find((row) => row.is_mine) ?? null;
  return (
    <>
      <div className="proj-top">
        <div className="proj-stat">
          <span className="eyebrow">Your draft</span>
          <span className="proj-stat-value">
            {mine === null ? "-" : `${fmt(mine.starters, 0)} pts`}
          </span>
          <span className="muted small">
            {mine === null
              ? "no seat in this draft"
              : mine.title_odds === null
                ? "Waiting for projected starters"
                : `${ordinal(mine.rank)} of ${rows.length} on projected starters`}
          </span>
        </div>
        <div className="proj-stat">
          <span className="eyebrow">Wins the league</span>
          <span className="proj-stat-value">
            {mine === null
              ? "-"
              : mine.title_odds === null
                ? "Not available"
                : pct(mine.title_odds)}
          </span>
          <span className="muted small">
            {mine?.title_odds === null
              ? "Odds need projected starters"
              : "4,000 seasons off these rosters"}
          </span>
        </div>
      </div>
      <section className="proj-card proj-table">
        <div className="proj-row proj-draft-row proj-head">
          <span>#</span>
          <span>Team</span>
          <span className="right">Starters</span>
          <span className="right">Bench</span>
          <span className="right">Wins it</span>
          <span>Still to fill</span>
        </div>
        {rows.map((row) => (
          <div
            key={row.slot}
            className={
              row.is_mine
                ? "proj-row proj-draft-row proj-body is-mine"
                : "proj-row proj-draft-row proj-body"
            }
          >
            <span className="muted num">{row.title_odds === null ? "-" : row.rank}</span>
            <span className="ellipsis">{row.name ?? `Slot ${row.slot}`}</span>
            <span className="right num proj-points">{fmt(row.starters, 0)}</span>
            <span className="right num muted">{fmt(row.bench, 0)}</span>
            <span className="right num">
              {row.title_odds === null ? "Not available" : pct(row.title_odds)}
            </span>
            <span className="muted ellipsis">{row.holes.join(", ") || "·"}</span>
          </div>
        ))}
      </section>
      <p className="muted small proj-note">
        Starters are the best lineup each roster can field from what it has drafted, scored on this
        league&apos;s own rules; bench is everyone else it took. &quot;Wins it&quot; is 4,000
        simulated seasons of those rosters, with the same week-to-week spread that prices a matchup
        on the Season screen, and it means finishing first on points: there is no schedule to
        simulate until the season starts. A roster with slots still to fill is projecting without
        them.
      </p>
    </>
  );
}

/** "3rd", for a rank read in a sentence rather than a column. */
function ordinal(rank: number): string {
  const tens = rank % 100;
  if (tens >= 11 && tens <= 13) return `${rank}th`;
  return `${rank}${["th", "st", "nd", "rd"][rank % 10] ?? "th"}`;
}

export function ProjectionsScreen({
  draft,
  season,
}: {
  draft: DraftView;
  season: SeasonView | null;
}) {
  const drafted = draft.draft_projections ?? [];
  const mine = season?.standings.find((row) => row.is_mine) ?? null;
  return (
    <div className="projections-screen">
      {drafted.length > 0 && <FromTheDraft rows={drafted} />}
      {season !== null && (
        <>
          <h2 className="proj-heading">From the season so far</h2>
          <div className="proj-top">
            <div className="proj-stat">
              <span className="eyebrow">Your projection</span>
              <span className="proj-stat-value">
                {mine === null ? "-" : fmt(mine.projected_points, 0)}
              </span>
              <span className="muted small">rest of season, best lineup each week</span>
            </div>
            <div className="proj-stat">
              <span className="eyebrow">Playoffs</span>
              <span className="proj-stat-value">
                {season.header.playoff_status ?? pct(season.header.playoff_odds)}
              </span>
              <span className="muted small">4,000 runs of the remaining schedule</span>
            </div>
          </div>
          <Week view={season} />
          <Table rows={season.standings} />
          <p className="muted small proj-note">
            Projected points are each roster&apos;s best lineup every remaining week, byes honoured.
            Playoff odds simulate the league&apos;s own bracket on its real schedule, with the same
            week-to-week spread that prices the matchup above.
          </p>
        </>
      )}
      {drafted.length === 0 && season === null && (
        <p className="muted">
          Nothing to project yet: the draft has no order posted and the season has not loaded.
        </p>
      )}
    </div>
  );
}
