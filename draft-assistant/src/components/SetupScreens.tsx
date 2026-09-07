// The two full-screen cards the app can be on before a draft is on screen:
// first launch, and restoring the league it had last time.
//
// Split out of Panels.tsx, which was closing on the 500-line cap; these two
// share nothing with the draft-screen panels but the stylesheet.

import { useState } from "react";
import { api } from "../api";
import { describeError } from "../errorText";
import { platformName } from "../leagues";
import type { DraftView, Platform } from "../types";

export function Setup({
  onReady,
  onConnectYahoo,
  onJoinHost,
  activeLeagueId = null,
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
  /** The league already on screen, when Settings opened this form to change
   *  the username. Prefilled, so a username can be saved on its own: the
   *  submit needed an id typed in, and the row that led here gave nothing to
   *  type it for. Null on first launch, where there is no league yet. */
  activeLeagueId?: string | null;
}) {
  const [username, setUsername] = useState("");
  const [leagueId, setLeagueId] = useState(activeLeagueId ?? "");
  const [busy, setBusy] = useState<string | null>(null);
  const [error, setError] = useState<string | null>(null);
  /** What is wrong with the username field, when Save was pressed over it
   *  empty. Cleared as soon as the field changes. */
  const [hint, setHint] = useState<string | null>(null);

  const canSubmit = leagueId.trim() !== "" && busy === null;
  // Re-loading the league on screen is how a new username takes effect, but
  // "Load league" over a field that already names it reads as a second copy.
  const saving = activeLeagueId !== null && leagueId.trim() === activeLeagueId;

  const submit = async () => {
    setError(null);
    // Save with nothing typed used to reload the whole league, ten seconds of
    // "Pulling league, players, and projections…" to change nothing. There is
    // nothing to save; say so and stay put. First launch is different: the
    // username is optional there, and Load league is loading a league.
    if (saving && username.trim() === "") {
      setHint("Type your Sleeper username to save it");
      return;
    }
    try {
      if (username.trim()) {
        setBusy("Looking up your Sleeper account…");
        await api.setMyUsername(username.trim());
      }
      setBusy("Pulling league, players, and projections…");
      onReady(await api.addLeague(leagueId.trim()));
    } catch (e) {
      // `String(e)` printed "Error: Error: the league id is not a number" on
      // the very first screen anyone sees. describeError is the one place
      // that prefix is stripped.
      setError(describeError(e));
      setBusy(null);
    }
  };

  return (
    // A form, so Enter in either field loads the league. Two text inputs and
    // a button that only the mouse could reach read as a broken screen to
    // anyone who types an id and presses Return.
    <form
      className="card-screen"
      onSubmit={(e) => {
        e.preventDefault();
        if (canSubmit) void submit();
      }}
    >
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
          onChange={(e) => {
            setUsername(e.target.value);
            setHint(null);
          }}
          placeholder="mcsleeper26"
          aria-invalid={hint !== null}
          aria-describedby={hint === null ? undefined : "username-hint"}
        />
      </label>
      {/* Outside the label, so the hint describes the field without being
          read as part of its name. */}
      {hint !== null && (
        <span className="error small" id="username-hint" role="alert">
          {hint}
        </span>
      )}
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
        <button type="submit" className="btn-primary card-screen-submit" disabled={!canSubmit}>
          {busy ?? (saving ? "Save" : "Load league")}
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
        First load pulls league, players and projections, about 10 seconds.
      </span>
      {/* Announced: the button goes back to saying "Load league" and the only
          other thing that changed was a line of red text further down. */}
      {error && (
        <div className="error" role="alert">
          {error}
        </div>
      )}
    </form>
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
  hostName = null,
  onLeaveHost,
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
  /** The host this window follows, when it is a follower. It is waiting on
   *  that Mac, not on Sleeper or Yahoo, and the card says so. */
  hostName?: string | null;
  /** Stop following the host. Offered in place of "Enter a different league",
   *  which a follower cannot do: its league is whatever the host has open. A
   *  follower whose host stopped answering had no way off this card at all. */
  onLeaveHost?: () => void;
}) {
  const reconnecting = lastError === null;
  const service = hostName ?? platformName(platform);
  return (
    <div className="card-screen">
      <h1>Draft Assistant</h1>
      {/* Polite: each attempt rewrites this line, and nothing else on the
          card moves. */}
      <div className="launch-status" role="status">
        <span className="launch-dot" />
        <span>
          {reconnecting
            ? `Connecting to ${service}`
            : `Reconnecting to ${service}, attempt ${attempt} of ${maxAttempts}`}
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
      </span>
      {/* Announced: it used to be a clause on the end of the line above, and
          a screen reader on the button heard nothing when a retry failed. */}
      {lastError !== null && (
        <span className="muted small launch-detail" role="alert">
          Last error: {lastError}
        </span>
      )}
      {!reconnecting && (
        <div className="launch-actions">
          <button type="button" className="btn-primary" onClick={onRetry}>
            Try again
          </button>
          {onLeaveHost === undefined ? (
            <button type="button" className="btn-ghost" onClick={onDifferentLeague}>
              Enter a different league
            </button>
          ) : (
            <button type="button" className="btn-ghost" onClick={onLeaveHost}>
              Leave host
            </button>
          )}
        </div>
      )}
    </div>
  );
}

// ---------- a follower whose host has nothing open ----------

/**
 * What a follower sees when the host has no league loaded.
 *
 * The shell used to fall through to the first-launch form here, whose every
 * button (a Sleeper league, Yahoo, joining a host) the follower's backend
 * refuses by name, and which had no "Leave host" anywhere on it. The two
 * things that can actually happen next are the host opening a league and
 * this Mac going back to its own.
 */
export function HostWaiting({
  hostName,
  onRetry,
  onLeaveHost,
}: {
  hostName: string;
  onRetry: () => void;
  onLeaveHost: () => void;
}) {
  return (
    <div className="card-screen">
      <h1>Draft Assistant</h1>
      <div className="launch-status">
        <span className="launch-dot" />
        <span>Following {hostName}</span>
      </div>
      <span className="muted small launch-detail">
        {hostName} has no league loaded. This screen shows whatever the host opens.
      </span>
      <div className="launch-actions">
        <button type="button" className="btn-primary" onClick={onRetry}>
          Try again
        </button>
        <button type="button" className="btn-ghost" onClick={onLeaveHost}>
          Leave host
        </button>
      </div>
    </div>
  );
}
