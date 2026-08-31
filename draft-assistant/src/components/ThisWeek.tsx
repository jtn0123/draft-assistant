// The season screen's main column: start/sit calls, the head-to-head lineup
// (table or scoreboard), and waiver targets.

import { useState } from "react";
import type { LineupCall, MatchupView, WaiverTarget } from "../season-types";
import { fmt, ideasAgeNote, injuryWord, kickoffLabel, pct, signed } from "../format";
import { Headshot, PlayerName, PanelHead, PosBadge, TeamAvatar, Empty, Segmented } from "./bits";

const LINEUP_VIEW_KEY = "da.lineupView";
type LineupView = "Table" | "Scoreboard";

function storedLineupView(): LineupView {
  try {
    return localStorage.getItem(LINEUP_VIEW_KEY) === "Scoreboard" ? "Scoreboard" : "Table";
  } catch {
    return "Table";
  }
}

// ---------- calls to make ----------

export function CallsToMake({
  calls,
  pointsOnTable,
}: {
  calls: LineupCall[];
  pointsOnTable: number;
}) {
  const [openIndex, setOpenIndex] = useState<number | null>(null);
  const [allOpen, setAllOpen] = useState(false);

  if (calls.length === 0) {
    return (
      <div className="calls is-clear">
        <span className="calls-title">Your lineup is already optimal</span>
        <span className="mid small">
          Every starting slot holds the best projected player on your roster.
        </span>
      </div>
    );
  }

  return (
    <div className="calls">
      <div className="calls-head">
        <span className="calls-title">
          {calls.length} call{calls.length === 1 ? "" : "s"} to make — {fmt(pointsOnTable, 1)}{" "}
          points on the table
        </span>
        <button
          type="button"
          className={allOpen ? "calls-all is-on" : "calls-all"}
          onClick={() => {
            setAllOpen((open) => !open);
            setOpenIndex(null);
          }}
        >
          {allOpen ? "Hide all reasons" : "Show all reasons"}
        </button>
      </div>
      {calls.map((call, index) => {
        const open = allOpen || openIndex === index;
        return (
          <div className="call" key={`${call.slot}-${call.player_in_id}`}>
            <button
              type="button"
              className="call-row"
              onClick={() => setOpenIndex(open ? null : index)}
              aria-expanded={open}
            >
              <span className="eyebrow">{call.slot}</span>
              <span className="call-players">
                <PlayerName
                  name={call.player_in}
                  team={call.player_in_team}
                  playerId={call.player_in_id}
                />
                <span className="muted small">over</span>
                <span className="mid ellipsis">{call.player_out}</span>
              </span>
              <span className="call-gain">{signed(call.gain)}</span>
              <span className="call-why">{open ? "Hide" : "Why"}</span>
            </button>
            <CallNote reason={call.reason} locksAtMs={call.locks_at_ms} />
            {open && <span className="mid call-reason">{call.why}</span>}
          </div>
        );
      })}
      <span className="mid call-foot">Set it on Sleeper — this app reads, it doesn't write.</span>
    </div>
  );
}

/** The always-visible line under a call: the one reason worth reading, and
 * when the decision stops being yours to make. */
function CallNote({ reason, locksAtMs }: { reason?: string | null; locksAtMs?: number | null }) {
  const deadline = locksAtMs ? kickoffLabel(locksAtMs) : "";
  if (!reason && !deadline) return null;
  return (
    <span className="call-note">
      {reason}
      {reason && deadline ? " · " : ""}
      {deadline && `decide by ${deadline}`}
    </span>
  );
}

/** The Q/D/O flag beside a starter, spelled out on hover. */
function injuryProps(code: string | null | undefined) {
  if (!code) return {};
  return { tag: code, tagTitle: injuryWord(code) };
}

// ---------- lineup comparison ----------

export function LineupCompare({
  matchup,
  winOdds,
}: {
  matchup: MatchupView | null;
  /** 0..1 chance of winning this week, shown beside the margin. */
  winOdds: number;
}) {
  const [view, setView] = useState<LineupView>(storedLineupView);

  const change = (next: LineupView) => {
    setView(next);
    try {
      localStorage.setItem(LINEUP_VIEW_KEY, next);
    } catch {
      // Preference is a nicety; failing to store it must not break the toggle.
    }
  };

  const [which, setWhich] = useState<"Best" | "Set">("Best");

  if (matchup === null) {
    return (
      <section className="lineup">
        <PanelHead title="Lineups, slot by slot" />
        <Empty>No matchup this week — you're on a bye.</Empty>
      </section>
    );
  }

  // "Best" is the lineup you should have; "Set" is the one you actually have.
  const best = which === "Best";
  const rows = best ? matchup.rows : matchup.set_rows;
  const mine = best ? matchup.my_projected : matchup.set_projected;
  const leftOnBench = matchup.my_projected - matchup.set_projected;
  const margin = mine - matchup.opp_projected;
  // Bar lengths are relative to the highest single projection on the board.
  const peak = Math.max(1, ...rows.map((r) => Math.max(r.my_points, r.opp_points)));

  return (
    <section className="lineup">
      <div className="lineup-head">
        <span className="lineup-head-titles">
          <span className="eyebrow">Lineups, slot by slot</span>
          <span className={leftOnBench >= 0.05 ? "lineup-flag" : "lineup-flag is-clear"}>
            {leftOnBench >= 0.05
              ? `${fmt(leftOnBench, 1)} sitting on your bench`
              : "your lineup is already your best"}
          </span>
        </span>
        <span className="lineup-head-controls">
          <Segmented
            options={["Best", "Set"] as const}
            value={which}
            onChange={setWhich}
            titles={{
              Best: "The lineup you should be starting",
              Set: "The lineup you actually have set on Sleeper",
            }}
            label="Which lineup"
          />
          <Segmented
            options={["Table", "Scoreboard"] as const}
            value={view}
            onChange={change}
            label="Lineup view"
          />
        </span>
      </div>

      <div className="score-head">
        <div className="score-side">
          <span className="eyebrow team-cell">
            <TeamAvatar avatar={matchup.my_avatar} name={matchup.my_name} />
            {matchup.my_name} · {best ? "best" : "as set"}
          </span>
          <span className="score-big">{fmt(mine, 1)}</span>
        </div>
        <span className={margin >= 0 ? "score-margin is-up" : "score-margin is-down"}>
          {signed(margin)} · {pct(winOdds)} to win
        </span>
        <div className="score-side is-right">
          <span className="eyebrow team-cell">
            {matchup.opp_name}
            <TeamAvatar avatar={matchup.opp_avatar} name={matchup.opp_name} />
          </span>
          <span className="score-big is-them">{fmt(matchup.opp_projected, 1)}</span>
        </div>
      </div>

      {view === "Table" ? (
        <div className="lineup-table">
          <div className="lineup-row lineup-table-head">
            <span />
            <span>Your player</span>
            <span className="right">Proj</span>
            <span className="center gap-head">Gap</span>
            <span>Proj</span>
            <span>Their player</span>
          </div>
          {rows.map((row, i) => (
            <div className="lineup-row" key={`${row.slot}-${i}`}>
              <span className="eyebrow">{row.slot}</span>
              <span className="lineup-player">
                <PlayerName
                  name={row.my_name || "—"}
                  team={row.my_team}
                  playerId={row.my_player_id}
                  {...injuryProps(row.my_injury)}
                />
              </span>
              <span className="right strong">{fmt(row.my_points, 1)}</span>
              <Lean margin={row.margin} />
              <span className="mid strong">{fmt(row.opp_points, 1)}</span>
              <span className="lineup-player mid">
                <PlayerName
                  name={row.opp_name || "—"}
                  team={row.opp_team}
                  playerId={row.opp_player_id}
                  {...injuryProps(row.opp_injury)}
                />
              </span>
            </div>
          ))}
        </div>
      ) : (
        <div className="scoreboard">
          {rows.map((row, i) => (
            <div className="scoreboard-row" key={`${row.slot}-${i}`}>
              <span className="eyebrow">{row.slot}</span>
              <div className="scoreboard-side is-mine">
                <span className="ellipsis scoreboard-name">
                  <Headshot playerId={row.my_player_id} team={row.my_team} name={row.my_name} />
                  {row.my_name || "—"}
                </span>
                <span className="strong">{fmt(row.my_points, 1)}</span>
                <span className="bar is-mine" style={{ width: barWidth(row.my_points, peak) }} />
              </div>
              <Lean margin={row.margin} />
              <div className="scoreboard-side">
                <span className="bar is-theirs" style={{ width: barWidth(row.opp_points, peak) }} />
                <span className="mid strong">{fmt(row.opp_points, 1)}</span>
                <span className="muted ellipsis scoreboard-name">
                  {row.opp_name || "—"}
                  <Headshot playerId={row.opp_player_id} team={row.opp_team} name={row.opp_name} />
                </span>
              </div>
            </div>
          ))}
        </div>
      )}
    </section>
  );
}

/** The gap for one slot, sitting between the two teams: signed from my side,
 * so a plus is a slot I am winning and a minus is one I am losing. */
function Lean({ margin }: { margin: number }) {
  if (Math.abs(margin) < 0.05) return <span className="lean is-even">—</span>;
  return <span className={margin > 0 ? "lean is-mine" : "lean is-theirs"}>{signed(margin)}</span>;
}

/** "$38 of $100 left" — or just what's left when the total is unknown. */
function budgetNote(left: number | null, total: number | null): string {
  if (left === null) return "no FAAB budget";
  const remaining = `$${Math.round(left)}`;
  return total === null ? `${remaining} left` : `${remaining} of $${Math.round(total)} left`;
}

function barWidth(points: number, peak: number): string {
  return `${Math.max(2, Math.round((points / peak) * 120))}px`;
}

// ---------- waivers ----------

export function Waivers({
  waivers,
  budgetLeft,
  budgetTotal,
  analysisAsOfSecs,
}: {
  waivers: WaiverTarget[];
  budgetLeft: number | null;
  budgetTotal: number | null;
  /** When the waiver search last ran; absent or recent says nothing. */
  analysisAsOfSecs?: number;
}) {
  const ideasAge = ideasAgeNote(analysisAsOfSecs);
  const budget = budgetNote(budgetLeft, budgetTotal);
  return (
    <section className="waivers">
      <PanelHead
        title="Worth a claim"
        note={ideasAge === null ? budget : `${budget} · ${ideasAge}`}
      />
      {waivers.length === 0 ? (
        <Empty>No free agent would crack your starting lineup — nothing worth spending on.</Empty>
      ) : (
        <div className="waiver-list">
          {waivers.map((w) => (
            <div className="waiver-row" key={w.player_id}>
              <PosBadge position={w.position} />
              <PlayerName name={w.name} team={w.team} playerId={w.player_id} />
              <span className="waiver-gain">+{pct(w.gain_fraction)}</span>
              <span className="muted waiver-bid">
                {w.suggested_bid === null ? "—" : `$${w.suggested_bid}`} ·{" "}
                {w.rivals === 0 ? "nobody" : `${w.rivals} rival${w.rivals === 1 ? "" : "s"}`}
              </span>
            </div>
          ))}
        </div>
      )}
      <span className="muted small">
        Gains are the lift to your best starting lineup. Bids are a share of what's left.
      </span>
    </section>
  );
}
