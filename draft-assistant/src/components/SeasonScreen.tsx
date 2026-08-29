import type { DraftView, LineupCheck } from "../types";
import { fmt, ordinal, pct } from "../format";
import { ThisWeek } from "./ThisWeek";
import { MatchupTable } from "./MatchupTable";
import { Waivers } from "./Waivers";
import { ByeWeeks } from "./ByeWeeks";
import { Standings } from "./Standings";
import { SeasonSoFar } from "./SeasonSoFar";
import { Activity } from "./Activity";
import { History } from "./History";
import { RosterCard } from "./RosterCard";
import { PlayerCardProvider } from "./PlayerCard";

/**
 * The season, week first: what to act on before kickoff on the left (the
 * lineup, the wire, trades), the league for reference on the right
 * (standings, results, roster). Nothing here is about picking players —
 * the draft screen is the other switch position.
 */
export function SeasonScreen({ view }: { view: DraftView }) {
  const matchup = view.this_week?.matchup ?? null;
  return (
    <PlayerCardProvider view={view}>
      <WeekBanner view={view} />

      {view.data_health.warnings.length > 0 && (
        <div className="warnings">{view.data_health.warnings.join(" · ")}</div>
      )}

      <div className="season-main">
        <div className="season-col season-primary">
          <ThisWeek view={view} />
          {matchup && (
            <section>
              <h2>Lineups</h2>
              <MatchupTable matchup={matchup} />
            </section>
          )}
          <Waivers view={view} />
          <Activity view={view} />
          <ByeWeeks view={view} />
        </div>
        <div className="season-col">
          <Standings view={view} />
          <SeasonSoFar view={view} />
          <RosterCard view={view} />
          <History view={view} />
        </div>
      </div>
    </PlayerCardProvider>
  );
}

/** The week at a glance where the draft clock used to be. */
function WeekBanner({ view }: { view: DraftView }) {
  const w = view.this_week;
  if (!w) {
    return (
      <div className="clock week-banner">
        <div className="clock-main">
          <span className="clock-status">Season</span>
          <span className="muted">
            No week on the calendar yet — the matchup and lineup check appear once Sleeper
            publishes the schedule.
          </span>
        </div>
      </div>
    );
  }
  const mine = view.draft.my_slot;
  const m = w.matchup;
  const standings = view.season?.standings ?? [];
  const record = standings.find((r) => r.slot === mine) ?? null;
  const place = record ? standings.indexOf(record) + 1 : null;
  const projected = view.projected_standings.findIndex((t) => t.slot === mine);
  const odds = view.playoff_odds.find((o) => o.slot === mine) ?? null;
  return (
    <>
      {w.lineup && <LineupAlert lineup={w.lineup} />}
      <div className="clock week-banner" aria-label={`Week ${w.week} summary`}>
        <div className="clock-cell">
          <span className="clock-label">Week</span>
          <span className="clock-big">{w.week}</span>
        </div>
        <div className="clock-main">
          {m ? (
            <>
              <span className="clock-status">
                vs {m.opponent_name ?? `Slot ${m.opponent_slot}`}
              </span>
              <span className="week-score">
                {fmt(m.my_points, 1)} – {fmt(m.opponent_points, 1)} · {pct(m.win_probability)} to
                win
              </span>
            </>
          ) : (
            <span className="clock-status">No matchup this week</span>
          )}
        </div>
        <div className="clock-cell">
          <span className="clock-label">Record</span>
          <span className="clock-big">{record ? `${record.wins}–${record.losses}` : "0–0"}</span>
          <span className="muted small-text">
            {place !== null
              ? `${ordinal(place)} of ${standings.length}`
              : projected >= 0
                ? `projected ${ordinal(projected + 1)} of ${view.projected_standings.length}`
                : ""}
          </span>
        </div>
        <div className="clock-cell week-odds">
          <span className="clock-label">Playoffs</span>
          <span className="clock-big">{odds ? pct(odds.playoff_odds) : "–"}</span>
        </div>
      </div>
    </>
  );
}

/** One line when the lineup on Sleeper is not the best one; nothing otherwise. */
function LineupAlert({ lineup }: { lineup: LineupCheck }) {
  const swaps = lineup.changes.filter((c) => c.out !== null).length;
  const gain = lineup.best_points - lineup.set_points;
  if (swaps === 0 && lineup.empty_slots.length === 0) return null;
  const parts = [
    lineup.empty_slots.length > 0 && `${lineup.empty_slots.join(", ")} empty`,
    swaps > 0 && `${swaps} swap${swaps === 1 ? "" : "s"}`,
    gain > 0 && `+${fmt(gain, 1)} on the table`,
  ].filter(Boolean);
  return (
    <div className="lineup-alert" role="status">
      Your lineup on Sleeper is not your best — {parts.join(" · ")}. See the lineup check.
    </div>
  );
}
