import { useState } from "react";
import { api } from "../api";
import type { DraftView, Recommendation } from "../types";
import { errorMessage, fmt, pct } from "../format";

// ---------- setup screen ----------

export function Setup({ onReady }: { onReady: (view: DraftView) => void }) {
  const [username, setUsername] = useState("");
  const [leagueId, setLeagueId] = useState("");
  const [busy, setBusy] = useState<string | null>(null);
  const [error, setError] = useState<string | null>(null);

  const submit = async () => {
    setError(null);
    try {
      if (username.trim()) {
        setBusy("Looking up your Sleeper account…");
        await api.setMyUsername(username.trim());
      }
      setBusy("Pulling league, players, and projections… (first load takes ~10 seconds)");
      const view = await api.addLeague(leagueId.trim());
      onReady(view);
    } catch (e) {
      setError(errorMessage(e));
      setBusy(null);
    }
  };

  return (
    <div className="setup">
      <h1>Draft Assistant</h1>
      <p className="muted">
        Read-only Sleeper second screen. You draft in Sleeper; this tracks every
        pick and tells you who to take.
      </p>
      <label>
        Sleeper username
        <input
          value={username}
          onChange={(e) => setUsername(e.target.value)}
          placeholder="e.g. mcsleeper26"
          autoFocus
        />
      </label>
      <label>
        League ID
        <input
          value={leagueId}
          onChange={(e) => setLeagueId(e.target.value)}
          placeholder="e.g. 1389710366300200960"
        />
      </label>
      <button disabled={!leagueId.trim() || busy !== null} onClick={submit}>
        {busy ?? "Load league"}
      </button>
      {error && <div className="error">{error}</div>}
    </div>
  );
}

// ---------- clock banner ----------

export function ClockBanner({ view }: { view: DraftView }) {
  const d = view.draft;
  const preDraft = d.status === "pre_draft" && d.total_picks_made === 0;
  const complete = d.status === "complete";
  const cls = d.is_my_pick ? "clock mine" : "clock";
  return (
    <div className={cls}>
      <div className="clock-cell">
        <span className="clock-label">Round</span>
        <span className="clock-big">{d.current_round}</span>
      </div>
      <div className="clock-cell">
        <span className="clock-label">Pick</span>
        <span className="clock-big">{d.current_pick}</span>
      </div>
      <div className="clock-main">
        {complete ? (
          <span className="clock-status">Draft complete</span>
        ) : preDraft ? (
          <span className="clock-status">Draft has not started</span>
        ) : d.is_my_pick ? (
          <span className="clock-status you">YOU ARE ON THE CLOCK</span>
        ) : (
          <>
            <span className="clock-status">
              On the clock: {d.on_clock_name ?? `Slot ${d.on_clock_slot}`}
            </span>
            {d.picks_until_mine !== null && (
              <span className="muted">
                {d.picks_until_mine} pick{d.picks_until_mine === 1 ? "" : "s"} until you
              </span>
            )}
          </>
        )}
      </div>
      <div className="clock-cell next-picks">
        <span className="clock-label">Your picks</span>
        <span className="next-pick-list">
          {d.my_next_picks.slice(0, 4).join(" · ") || "–"}
        </span>
      </div>
    </div>
  );
}

// ---------- recommendation cards ----------

export function RecCard({ rec, onDraft }: { rec: Recommendation; onDraft: (id: string, name: string) => void }) {
  return (
    <div className={`rec ${rec.mode}`}>
      <div className="rec-head">
        <span className="rec-mode">{rec.mode}</span>
        <span className={`pos-badge pos-${rec.position}`}>{rec.position}</span>
      </div>
      <div className="rec-name">{rec.name}</div>
      <div className="rec-stats">
        {fmt(rec.points)} pts · VORP {fmt(rec.vorp)} · Tier {rec.tier}
        {rec.survival_next !== null && <> · survives {pct(rec.survival_next)}</>}
      </div>
      <ul className="rec-reasons">
        {rec.reasons.slice(0, 3).map((r, i) => (
          <li key={i}>{r}</li>
        ))}
      </ul>
      <button className="ghost" onClick={() => onDraft(rec.player_id, rec.name)}>
        Mark drafted
      </button>
    </div>
  );
}

// ---------- side panel ----------

export function SidePanel({ view }: { view: DraftView }) {
  const roster = view.my_roster;
  const starters = view.league.roster_positions.filter((s) => s !== "BN");
  const benchSize = view.league.roster_positions.filter((s) => s === "BN").length;
  return (
    <aside className="side">
      <section>
        <h3>My roster</h3>
        {roster === null ? (
          <p className="muted">Set your Sleeper username to track your team.</p>
        ) : roster.players.length === 0 ? (
          <p className="muted">No picks yet.</p>
        ) : (
          <ul className="roster">
            {roster.players.map((p) => (
              <li key={p.player_id}>
                <span className={`pos-badge pos-${p.position}`}>{p.position}</span>
                <span>{p.name}</span>
                <span className="muted">R{p.round}</span>
              </li>
            ))}
          </ul>
        )}
        {roster !== null && roster.open_starters.length > 0 && (
          <p className="muted small-text">
            Open starters:{" "}
            {roster.open_starters.map(([slot, n]) => `${slot}×${n}`).join(", ")} ·{" "}
            {starters.length} starters + {benchSize} bench
          </p>
        )}
      </section>
      <section>
        <h3>Tier alerts</h3>
        <ul className="alerts">
          {view.tier_alerts.map((a) => (
            <li key={a.position} className={a.players_left <= 2 ? "urgent" : ""}>
              <span className={`pos-badge pos-${a.position}`}>{a.position}</span>
              <span>Tier {a.tier}</span>
              <span className={a.players_left <= 2 ? "strong" : "muted"}>
                {a.players_left > 25 ? "25+" : a.players_left} left
              </span>
            </li>
          ))}
        </ul>
        {view.position_run && (
          <p className="run">🔥 {view.position_run} run in progress</p>
        )}
      </section>
      <section>
        <h3>Recent picks</h3>
        <ul className="recent">
          {view.recent_picks.map((p) => (
            <li key={p.pick_no}>
              <span className="muted">{p.pick_no}.</span> {p.name}
              <span className="muted"> · {p.position} · slot {p.slot}</span>
            </li>
          ))}
          {view.recent_picks.length === 0 && <li className="muted">None yet.</li>}
        </ul>
      </section>
    </aside>
  );
}
