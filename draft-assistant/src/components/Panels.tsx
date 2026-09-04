// Draft-screen panels: the three recommendation cards and the left rail
// (roster, at-risk players, tier alerts, pick market, recent picks).

import { useMemo, useState } from "react";
import { api } from "../api";
import { platformName } from "../leagues";
import type { DraftView, PickPrice, Platform, Recommendation } from "../types";
import { fmt, pct, pickLabel, posRank, signed } from "../format";
import { PlayerName, PosBadge, PanelHead, Empty } from "./bits";

// ---------- setup ----------

export function Setup({
  onReady,
  onConnectYahoo,
  onJoinHost,
}: {
  onReady: (view: DraftView) => void;
  /** Open the Yahoo connect dialog instead. A Yahoo player has no Sleeper
   *  league id to paste, and this screen used to be the only way in — so the
   *  app was unusable for them until a Sleeper league had been loaded first. */
  onConnectYahoo: () => void;
  /** Join a Draft Assistant already running on the network instead of
   *  loading a league here. Someone handed a second screen an app with no
   *  league of its own used to have to set one up before they could watch. */
  onJoinHost: () => void;
}) {
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
          A read-only second screen for Sleeper and Yahoo. You draft there; this tracks every pick
          and says who to take.
        </p>
      </div>
      <label className="field">
        Sleeper username
        <input
          className="text-input"
          value={username}
          onChange={(e) => setUsername(e.target.value)}
          placeholder="mcsleeper26"
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
      <div className="launch-actions">
        <button
          type="button"
          className="btn-primary card-screen-submit"
          disabled={!leagueId.trim() || busy !== null}
          onClick={() => void submit()}
        >
          {busy ?? "Load league"}
        </button>
        <button
          type="button"
          className="btn-ghost"
          disabled={busy !== null}
          onClick={onConnectYahoo}
        >
          Connect Yahoo instead
        </button>
        <button type="button" className="btn-ghost" disabled={busy !== null} onClick={onJoinHost}>
          Join another Draft Assistant…
        </button>
      </div>
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
  platform,
  attempt,
  maxAttempts,
  lastError,
  onRetry,
  onDifferentLeague,
}: {
  leagueName: string | null;
  leagueId: string | null;
  /** Which service the league being restored is read from, so the screen
   *  names the one it is actually waiting on. */
  platform: Platform;
  attempt: number;
  maxAttempts: number;
  lastError: string | null;
  onRetry: () => void;
  onDifferentLeague: () => void;
}) {
  const reconnecting = lastError === null;
  const service = platformName(platform);
  return (
    <div className="card-screen">
      <h1>Draft Assistant</h1>
      <div className="launch-status">
        <span className="launch-dot" />
        <span>
          {reconnecting
            ? `Connecting to ${service}`
            : `Reconnecting to ${service} — attempt ${attempt} of ${maxAttempts}`}
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
  // Several hundred players, filtered and sorted, on a panel that re-renders
  // on every 3-second poll and every tick of the pick clock. `applyView`
  // recycles the `available` array when nothing about the pool changed
  // (boardIdentity.ts), so this memo genuinely holds across those updates.
  const atRisk = useMemo(
    () =>
      view.available
        .filter((p) => p.survival_next !== null && p.survival_next < 0.5)
        .sort((a, b) => (a.survival_next ?? 1) - (b.survival_next ?? 1))
        .slice(0, 5),
    [view.available],
  );
  // Survival is judged at my next pick AFTER the one I'm making now, which is
  // what the backend computed `survival_next` against — the label has to name
  // the same pick, in the same round.pick form used everywhere else.
  const survivalPick = survivalTargetPick(view.draft);

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
                <span className="muted" title={p.is_keeper ? "Kept from last season" : undefined}>
                  R{p.round}
                  {p.is_keeper ? " · K" : ""}
                </span>
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
                {/* What his going costs you, so it is always a loss. Written
                    with the shared signed helper rather than a minus sign glued
                    to the number: a player whose VORP is already negative used
                    to read "−-4". */}
                <span className="mid num">{signed(-Math.abs(p.vorp), 0)}</span>
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

      <PickMarket prices={view.pick_prices ?? []} />

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

/** The going rate for a pick in each round, in the only currency the rest of
 * the screen speaks: points over replacement.
 *
 * Rendered only once the draft has rounds to learn from — before that the
 * backend sends nothing rather than guessing, and an empty panel would just be
 * a heading. The list is monotone by construction (`pick_value.rs` caps each
 * round at the one before it), so it always reads top to bottom as cheaper. */
function PickMarket({ prices }: { prices: PickPrice[] }) {
  if (prices.length === 0) return null;
  return (
    <section className="panel">
      <PanelHead title="Pick market" note={<span title={PRICE_NOTE}>VORP pts</span>} />
      <div className="price-list">
        {prices.map((price) => (
          <div className="price-row" key={price.round} title={PRICE_NOTE}>
            <span className="muted num">R{price.round}</span>
            <span className="mid ellipsis">{price.example ?? "—"}</span>
            <span className="num price-points">{fmt(price.points)}</span>
          </div>
        ))}
      </div>
    </section>
  );
}

/** Said in full wherever the number is shown, because "R7 · 12" on its own
 * invites being read as a projection rather than a price. */
const PRICE_NOTE =
  "The median VORP taken in this round of this league's draft — what a pick " +
  "there has actually been worth, and who went at that price.";

/**
 * The pick the backend priced survival against, so the headline over the
 * at-risk list names that pick and not another one.
 *
 * MIRRORS `view_signals::survival_target` in src-tauri, including the part
 * that is easy to miss: at a snake turn I hold two picks with nothing in
 * between, and those count as one window. Taking "the next one after this"
 * literally named pick 1.13 while the backend had priced everything against
 * 3.12 — the one moment the board is most dangerous, read as the safest.
 */
function survivalTargetPick(d: DraftView["draft"]): number | null {
  const later = d.my_next_picks.filter((pick) => !d.is_my_pick || pick !== d.current_pick);
  const next = later[0];
  if (next === undefined) return null;
  const after = later[1];
  return after !== undefined && next === d.current_pick + 1 ? after : next;
}

/** The design only alarms a survival chance once it drops to a quarter. */
function riskClass(survival: number | null): string {
  return survival !== null && survival <= 0.25 ? "num risk-surv is-low" : "num risk-surv mid";
}
