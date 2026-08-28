import { useEffect, useState, type FormEvent } from "react";
import { api } from "../api";
import type { DraftView, Recommendation } from "../types";
import { errorMessage, fmt, pct } from "../format";

// ---------- setup screen ----------

export function Setup({ onReady }: { onReady: (view: DraftView) => void }) {
  const [username, setUsername] = useState("");
  const [leagueId, setLeagueId] = useState("");
  const [busy, setBusy] = useState<string | null>(null);
  const [error, setError] = useState<string | null>(null);

  const submit = async (event?: FormEvent) => {
    event?.preventDefault();
    if (!leagueId.trim() || busy !== null) return;
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
    <form className="setup" onSubmit={submit}>
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
      <button type="submit" disabled={!leagueId.trim() || busy !== null}>
        {busy ?? "Load league"}
      </button>
      {error && <div className="error">{error}</div>}
    </form>
  );
}

// ---------- clock banner ----------

/** Wall-clock time, re-read every second while `active`. */
function useNow(active: boolean): number {
  const [now, setNow] = useState(() => Date.now());
  useEffect(() => {
    if (!active) return;
    const timer = window.setInterval(() => setNow(Date.now()), 1000);
    return () => window.clearInterval(timer);
  }, [active]);
  return now;
}

function formatCountdown(ms: number): string {
  const total = Math.max(0, Math.ceil(ms / 1000));
  const minutes = Math.floor(total / 60);
  const seconds = total % 60;
  return `${minutes}:${seconds.toString().padStart(2, "0")}`;
}

function formatStart(ms: number, now: number): string {
  const at = new Date(ms).toLocaleTimeString([], { hour: "numeric", minute: "2-digit" });
  // Sleeper leaves a draft in pre_draft past its scheduled time until the
  // commissioner starts it, and "starts 5:00 PM" at 5:20 reads like a bug.
  return ms <= now ? `scheduled for ${at} — waiting on the commissioner` : `starts ${at}`;
}

export function ClockBanner({ view }: { view: DraftView }) {
  const d = view.draft;
  // Not "no picks made": a keeper league starts with picks already in the book
  // (this one with 25), and counting those had the app announcing someone on
  // the clock hours before the draft. Nothing has been *played* until the
  // clock has moved off pick 1.
  const preDraft = d.status === "pre_draft" && d.current_pick === 1;
  const complete = d.status === "complete";
  const cls = d.is_my_pick ? "clock mine" : "clock";
  const now = useNow(d.pick_deadline !== null || preDraft);
  const remaining = d.pick_deadline === null ? null : d.pick_deadline - now;
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
      {/* Only the status text is a live region: the countdown below changes
          every second, and inside here a screen reader would re-read the whole
          banner on every tick. */}
      <div className="clock-main" role="status" aria-live="polite">
        {complete ? (
          <span className="clock-status">Draft complete</span>
        ) : preDraft ? (
          <span className="clock-status">
            Draft has not started
            {d.start_time !== null && ` · ${formatStart(d.start_time, now)}`}
          </span>
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
      {remaining !== null && (
        <div className="clock-cell">
          <span className="clock-label">Clock</span>
          <span
            className={`clock-big${remaining <= 10_000 ? " urgent" : ""}`}
            aria-label="Pick clock"
          >
            {formatCountdown(remaining)}
          </span>
        </div>
      )}
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
        <h2>My roster</h2>
        {roster === null ? (
          <p className="muted">Set your Sleeper username to track your team.</p>
        ) : roster.players.length === 0 ? (
          <p className="muted">No picks yet.</p>
        ) : (
          <ul className="roster" aria-label="My roster">
            {roster.players.map((p) => (
              <li key={p.player_id}>
                <span className={`pos-badge pos-${p.position}`}>{p.position}</span>
                <span>
                  {p.name}
                  {p.is_keeper && (
                    <span className="keeper-tag" title="Kept from last season, not drafted tonight">
                      keeper
                    </span>
                  )}
                </span>
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
        <h2>Tier alerts</h2>
        <ul className="alerts">
          {view.tier_alerts.map((a) => (
            <li
              key={a.position}
              className={a.players_left <= 2 ? "urgent" : ""}
              aria-label={a.position}
              // Tier numbers run past 14 now that bands are split by spread, so
              // say what the number means instead of leaving "Tier 7" to be
              // read as a ranking against another position's "Tier 1".
              title={`The best ${a.position} band still on the board is tier ${a.tier}; ${a.players_left} left in it`}
            >
              <span className={`pos-badge pos-${a.position}`}>{a.position}</span>
              <span>
                Top tier <span className="muted">T{a.tier}</span>
              </span>
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
        <h2>Recent picks</h2>
        <ul className="recent">
          {view.recent_picks.map((p) => (
            <li key={p.pick_no}>
              <span className="muted">{p.pick_no}.</span> {p.name}
              <span className="muted">
                {" "}· {p.position} · {p.slot_name ?? `slot ${p.slot}`}
              </span>
            </li>
          ))}
          {view.recent_picks.length === 0 && <li className="muted">None yet.</li>}
        </ul>
      </section>
    </aside>
  );
}
