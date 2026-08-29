import type { DraftView, LineupChange, Starter } from "../types";
import { fmt, pct } from "../format";
import { PlayerName } from "./PlayerCard";

/**
 * The week ahead. Two things the user can act on before kickoff: slots where
 * the lineup set on Sleeper is not the best one (the app cannot set it — it
 * says what to set), and who they play, with a margin and odds.
 */
export function ThisWeek({ view }: { view: DraftView }) {
  const w = view.this_week;
  if (!w) return null;
  const { lineup, matchup } = w;
  return (
    <section className="this-week" aria-label={`Week ${w.week}`}>
      <h2>Lineup check</h2>
      {lineup && (
        <div className="lineup-check">
          {lineup.changes.length === 0 && lineup.empty_slots.length === 0 ? (
            <p className="ok">Your Sleeper lineup is the best one — {fmt(lineup.best_points, 1)} projected.</p>
          ) : lineup.changes.length === 0 ? (
            <>
              <p className="ok">Best lineup set — {fmt(lineup.best_points, 1)} projected.</p>
              <p className="lineup-empty">
                {lineup.empty_slots.join(", ")} empty and nobody on the roster fills it — see
                waiver targets.
              </p>
            </>
          ) : (
            <>
              <p className="strong">
                Lineup on Sleeper: {fmt(lineup.set_points, 1)} · best {fmt(lineup.best_points, 1)}{" "}
                <span className="gain">(+{fmt(lineup.best_points - lineup.set_points, 1)})</span>
              </p>
              <ul className="lineup-changes" aria-label="Lineup changes">
                {lineup.changes.map((c) => (
                  <li key={c.slot + c.in_.player_id} className={c.out ? "" : "empty"}>
                    <span className="slot">{c.slot}</span>
                    <span>{describe(c)}</span>
                    <span className="gain">+{fmt(c.gain, 1)}</span>
                  </li>
                ))}
              </ul>
            </>
          )}
        </div>
      )}
      {matchup && (
        <div className="matchup" title={matchupTitle(matchup.my_starters, matchup.opponent_starters)}>
          <span>
            vs <strong>{matchup.opponent_name ?? `Slot ${matchup.opponent_slot}`}</strong>
          </span>
          <span className="matchup-score">
            {fmt(matchup.my_points, 1)} – {fmt(matchup.opponent_points, 1)}
          </span>
          <span className={matchup.margin >= 0 ? "surv" : "surv low"}>
            {pct(matchup.win_probability)} to win
          </span>
        </div>
      )}
    </section>
  );
}

function describe(c: LineupChange) {
  const in_ = (
    <>
      <PlayerName id={c.in_.player_id}>{c.in_.name}</PlayerName> ({fmt(c.in_.points, 1)})
    </>
  );
  if (!c.out) return <>empty — start {in_}</>;
  const out = <PlayerName id={c.out.player_id}>{c.out.name}</PlayerName>;
  if (c.out.injury && c.out.points === 0) {
    return (
      <>
        {out} is {c.out.injury} — start {in_}
      </>
    );
  }
  return (
    <>
      {in_} over {out} ({fmt(c.out.points, 1)})
    </>
  );
}

function matchupTitle(mine: Starter[], theirs: Starter[]): string {
  const line = (xs: Starter[]) => xs.map((s) => `${s.slot} ${s.name} ${fmt(s.points, 1)}`).join("\n");
  return `You\n${line(mine)}\n\nThem\n${line(theirs)}`;
}
