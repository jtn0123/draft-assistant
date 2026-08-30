// Draft-screen panels: the three recommendation cards and the left rail
// (roster, at-risk players, tier alerts, recent picks).

import { useState } from "react";
import { api } from "../api";
import type { DraftView, Recommendation } from "../types";
import { fmt, pct, pickLabel, posRank } from "../format";
import { PlayerName, PosBadge, PanelHead, Empty } from "./bits";

// ---------- setup ----------

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
      setBusy("Pulling league, players, and projections…");
      onReady(await api.addLeague(leagueId.trim()));
    } catch (e) {
      setError(String(e));
      setBusy(null);
    }
  };

  return (
    <div className="card-screen">
      <div className="card-screen-intro">
        <h1>Draft Assistant</h1>
        <p className="mid">
          A read-only second screen for Sleeper. You draft in Sleeper; this tracks every pick and
          says who to take.
        </p>
      </div>
      <label className="field">
        Sleeper username
        <input
          className="text-input"
          value={username}
          onChange={(e) => setUsername(e.target.value)}
          placeholder="mcsleeper26"
          autoFocus
        />
      </label>
      <label className="field">
        League ID
        <input
          className="text-input"
          value={leagueId}
          onChange={(e) => setLeagueId(e.target.value)}
          placeholder="1389710366300200960"
        />
      </label>
      <button
        type="button"
        className="btn-primary card-screen-submit"
        disabled={!leagueId.trim() || busy !== null}
        onClick={submit}
      >
        {busy ?? "Load league"}
      </button>
      <span className="muted small">
        First load pulls league, players and projections — about 10 seconds.
      </span>
      {error && <div className="error">{error}</div>}
    </div>
  );
}

// ---------- launch / reconnect ----------

export function LaunchScreen({
  leagueName,
  leagueId,
  attempt,
  maxAttempts,
  lastError,
  onRetry,
  onDifferentLeague,
}: {
  leagueName: string | null;
  leagueId: string | null;
  attempt: number;
  maxAttempts: number;
  lastError: string | null;
  onRetry: () => void;
  onDifferentLeague: () => void;
}) {
  const reconnecting = lastError === null;
  return (
    <div className="card-screen">
      <h1>Draft Assistant</h1>
      <div className="launch-status">
        <span className="launch-dot" />
        <span>
          {reconnecting
            ? "Connecting to Sleeper"
            : `Reconnecting to Sleeper — attempt ${attempt} of ${maxAttempts}`}
        </span>
      </div>
      <span className="muted small launch-detail">
        {leagueName === null ? (
          leagueId === null ? (
            "Restoring your last league."
          ) : (
            `Restoring league ${leagueId}.`
          )
        ) : (
          <>
            Restoring <strong className="mid">{leagueName}</strong>
            {leagueId !== null && ` (${leagueId})`}.
          </>
        )}
        {lastError !== null && ` Last error: ${lastError}`}
      </span>
      {!reconnecting && (
        <div className="launch-actions">
          <button type="button" className="btn-primary" onClick={onRetry}>
            Try again
          </button>
          <button type="button" className="btn-ghost" onClick={onDifferentLeague}>
            Enter a different league
          </button>
        </div>
      )}
    </div>
  );
}

// ---------- recommendation cards ----------

/** The design labels the three modes Safe / Balanced / Upside. */
const MODE_LABEL: Record<string, string> = {
  safe: "Safe",
  balanced: "Balanced",
  upside: "Upside",
};

export function RecCard({
  rec,
  featured,
  positionRank,
  onDraft,
}: {
  rec: Recommendation;
  featured: boolean;
  positionRank: number | null;
  onDraft: (id: string, name: string) => void;
}) {
  return (
    <div className={featured ? "rec is-featured" : "rec"}>
      <div className="rec-head">
        <span className={featured ? "rec-mode is-featured" : "rec-mode"}>
          {MODE_LABEL[rec.mode] ?? rec.mode}
        </span>
        <span className={`pos-badge pos-${rec.position}`}>
          {posRank(rec.position, positionRank)}
        </span>
      </div>
      <span className="rec-name">
        <PlayerName name={rec.name} team={rec.team} playerId={rec.player_id} />
      </span>
      <span className="mid rec-stats num">
        {fmt(rec.points)} pts · VORP {fmt(rec.vorp)} · tier {rec.tier}
        {rec.survival_next !== null && ` · survives ${pct(rec.survival_next)}`}
      </span>
      <ul className="rec-reasons">
        {rec.reasons.slice(0, 2).map((reason, i) => (
          <li key={i}>{reason}</li>
        ))}
      </ul>
      <button
        type="button"
        className={featured ? "btn-primary rec-action" : "btn-ghost rec-action"}
        onClick={() => onDraft(rec.player_id, rec.name)}
      >
        Mark drafted
      </button>
    </div>
  );
}

// ---------- left rail ----------

export function SidePanel({ view }: { view: DraftView }) {
  const roster = view.my_roster;
  const rounds = view.draft.rounds;
  const atRisk = view.available
    .filter((p) => p.survival_next !== null && p.survival_next < 0.5)
    .sort((a, b) => (a.survival_next ?? 1) - (b.survival_next ?? 1))
    .slice(0, 5);
  // Survival is judged at my next pick AFTER the one I'm making now, which is
  // what the backend computed `survival_next` against — the label has to name
  // the same pick, in the same round.pick form used everywhere else.
  const survivalPick =
    (view.draft.is_my_pick ? view.draft.my_next_picks[1] : view.draft.my_next_picks[0]) ?? null;

  return (
    <aside className="rail">
      <section className="panel">
        <PanelHead
          title="My roster"
          note={roster === null ? undefined : `${roster.players.length} of ${rounds}`}
        />
        {roster === null ? (
          <Empty>Set your Sleeper username to track your team.</Empty>
        ) : roster.players.length === 0 ? (
          <Empty>No picks yet.</Empty>
        ) : (
          <ul className="roster-list">
            {roster.players.map((p) => (
              <li key={p.player_id}>
                <span className="roster-player">
                  <PosBadge position={p.position} />
                  <PlayerName name={p.name} team={p.team} playerId={p.player_id} />
                </span>
                <span className="muted">R{p.round}</span>
              </li>
            ))}
          </ul>
        )}
        {roster !== null && roster.open_starters.length > 0 && (
          <span className="muted small">
            Open starters: {roster.open_starters.map(([slot, n]) => `${slot}×${n}`).join(", ")}
          </span>
        )}
      </section>

      {atRisk.length > 0 && (
        <section className="panel">
          <PanelHead
            title={
              survivalPick === null
                ? "Won't last"
                : `Won't last to ${pickLabel(survivalPick, view.draft.teams)}`
            }
          />
          <div className="risk-list">
            {atRisk.map((p) => (
              <div className="risk-row" key={p.player_id}>
                <PosBadge position={p.position} />
                <PlayerName name={p.name} team={p.team} playerId={p.player_id} />
                <span className={riskClass(p.survival_next)}>{pct(p.survival_next)}</span>
                <span className="mid num">−{fmt(p.vorp)}</span>
              </div>
            ))}
          </div>
        </section>
      )}

      <section className="panel">
        <PanelHead title="Tier alerts" />
        <div className="alert-list">
          {view.tier_alerts.map((a) => (
            <div className="alert-row" key={a.position}>
              <PosBadge position={a.position} />
              <span>
                Top tier <span className="muted">T{a.tier}</span>
              </span>
              <span className={a.players_left <= 2 ? "alert-count is-urgent" : "mid"}>
                {a.players_left > 25 ? "25+" : a.players_left} left
              </span>
            </div>
          ))}
          {view.tier_alerts.length === 0 && <Empty>No players left on the board.</Empty>}
        </div>
        {view.position_run && (
          <span className="run-note">
            {view.position_run.position} run in progress — {view.position_run.count} of the last{" "}
            {view.position_run.window}
          </span>
        )}
      </section>

      <section className="panel">
        <PanelHead title="Recent picks" />
        <div className="recent-list">
          {view.recent_picks.map((p) => (
            <span className="recent-row" key={p.pick_no}>
              <span className="muted num">{pickLabel(p.pick_no, view.draft.teams)}</span>{" "}
              <PlayerName name={p.name} team={p.team} playerId={p.player_id} />{" "}
              <span className="muted">
                · {p.position} · {p.slot_name ?? `slot ${p.slot}`}
              </span>
            </span>
          ))}
          {view.recent_picks.length === 0 && <Empty>None yet.</Empty>}
        </div>
      </section>
    </aside>
  );
}

/** The design only alarms a survival chance once it drops to a quarter. */
function riskClass(survival: number | null): string {
  return survival !== null && survival <= 0.25 ? "num risk-surv is-low" : "num risk-surv mid";
}
