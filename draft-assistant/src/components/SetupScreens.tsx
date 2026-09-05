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
      // `String(e)` printed "Error: Error: the league id is not a number" on
      // the very first screen anyone sees. describeError is the one place
      // that prefix is stripped.
      setError(describeError(e));
      setBusy(null);
    }
  };

  const canSubmit = leagueId.trim() !== "" && busy === null;

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
        <button type="submit" className="btn-primary card-screen-submit" disabled={!canSubmit}>
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
