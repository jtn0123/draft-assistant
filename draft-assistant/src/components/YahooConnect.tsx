// Connect a Yahoo account, in one dialog with three steps.
//
// Sleeper needs no account at all: paste an id and the board builds. Yahoo
// refuses every call without a user token, even for a public league, so there
// is a login to get through before a Yahoo league can be listed at all. The
// three steps are the three things that can be missing, and which one is on
// screen is read off the status rather than kept as its own state:
//
//   no credentials  -> the app you registered at developer.yahoo.com
//   no token        -> the browser round trip that gets one
//   connected       -> your account, and the leagues it plays in
//
// The secret is write-only. It goes in, the backend puts it in the keychain,
// and nothing ever hands it back — `configured` is all the UI is told.

import { useEffect, useRef, useState } from "react";
import { api } from "../api";
import type { StoredLeague, YahooStatus } from "../types";
import { LeagueRow } from "./LeagueRow";
import { useFocusTrap } from "./useFocusTrap";

/** Errors arrive from the backend as strings; this is how the picker reads
 *  them out, and this dialog reads them the same way. */
function reason(e: unknown): string {
  return String(e).replace(/^Error:\s*/, "");
}

export function YahooConnect({
  activeId,
  busy,
  onSwitch,
  onStatus,
  onClose,
}: {
  /** The league on screen, so its row in the Yahoo list is marked. */
  activeId: string | null;
  /** True while a league switch is in flight. */
  busy: boolean;
  onSwitch: (leagueId: string) => void;
  /** Every status the backend hands back, so the shell's settings row and the
   *  picker's Yahoo lookup follow along without asking again. */
  onStatus: (status: YahooStatus) => void;
  onClose: () => void;
}) {
  const dialog = useRef<HTMLDivElement>(null);
  const opener = useRef<HTMLElement | null>(null);
  const [status, setStatus] = useState<YahooStatus | null>(null);
  const [working, setWorking] = useState(false);
  const [error, setError] = useState<string | null>(null);
  const [clientId, setClientId] = useState("");
  const [clientSecret, setClientSecret] = useState("");
  const [start, setStart] = useState<{ authorize_url: string; state: string } | null>(null);
  const [code, setCode] = useState("");
  const [copied, setCopied] = useState(false);
  const [leagues, setLeagues] = useState<StoredLeague[] | null>(null);

  useEffect(() => {
    opener.current = document.activeElement as HTMLElement | null;
    return () => {
      opener.current?.focus();
      opener.current = null;
    };
  }, []);

  useFocusTrap(dialog, onClose);

  // Nothing is set synchronously here: the first status lands from the
  // promise, and until it does the dialog says it is still asking.
  useEffect(() => {
    let cancelled = false;
    api
      .yahooStatus()
      .then((next) => {
        if (cancelled) return;
        setStatus(next);
        onStatus(next);
      })
      .catch((e: unknown) => {
        if (!cancelled) setError(reason(e));
      });
    return () => {
      cancelled = true;
    };
    // Deliberately once: re-asking because the shell handed down a fresh
    // callback would throw away a step the user is halfway through.
    // eslint-disable-next-line react-hooks/exhaustive-deps
  }, []);

  /** Run one backend call, keeping the whole dialog still while it is out. */
  const run = async (what: () => Promise<void>) => {
    setWorking(true);
    setError(null);
    try {
      await what();
    } catch (e) {
      setError(reason(e));
    } finally {
      setWorking(false);
    }
  };

  const take = (next: YahooStatus) => {
    setStatus(next);
    onStatus(next);
  };

  const save = () =>
    run(async () => {
      take(await api.yahooSaveCredentials(clientId.trim(), clientSecret.trim()));
      setClientSecret("");
    });

  const begin = () =>
    run(async () => {
      const started = await api.yahooBeginConnect();
      setStart({ authorize_url: started.authorize_url, state: started.state });
      setCopied(false);
    });

  const finish = () =>
    run(async () => {
      if (start === null) return;
      take(await api.yahooFinishConnect(code.trim(), start.state));
      setStart(null);
      setCode("");
    });

  const disconnect = () =>
    run(async () => {
      take(await api.yahooDisconnect());
      setLeagues(null);
      setStart(null);
    });

  const findLeagues = () =>
    run(async () => {
      setLeagues(await api.yahooLeagues());
    });

  const copy = () => {
    if (start === null) return;
    void navigator.clipboard
      ?.writeText(start.authorize_url)
      .then(() => setCopied(true))
      .catch((e: unknown) => setError(reason(e)));
  };

  return (
    <div
      className="scrim"
      role="presentation"
      onClick={(e) => {
        if (e.target === e.currentTarget) onClose();
      }}
    >
      <div
        className="dialog"
        ref={dialog}
        role="dialog"
        aria-modal="true"
        aria-labelledby="yahoo-connect-title"
      >
        <span className="eyebrow">Yahoo</span>
        <span className="dialog-title" id="yahoo-connect-title">
          Connect Yahoo Fantasy
        </span>

        {status === null ? (
          <span className="mid dialog-note">Checking the Yahoo connection…</span>
        ) : !status.configured ? (
          <Credentials
            clientId={clientId}
            clientSecret={clientSecret}
            redirect={status.redirect}
            working={working}
            onClientId={setClientId}
            onClientSecret={setClientSecret}
            onSave={() => void save()}
          />
        ) : !status.connected ? (
          <Connect
            start={start}
            code={code}
            copied={copied}
            working={working}
            onBegin={() => void begin()}
            onCode={setCode}
            onCopy={copy}
            onFinish={() => void finish()}
          />
        ) : (
          <Connected
            account={status.account}
            leagues={leagues}
            activeId={activeId}
            busy={busy || working}
            onFind={() => void findLeagues()}
            onSwitch={onSwitch}
            onDisconnect={() => void disconnect()}
          />
        )}

        {error !== null && <div className="error">{error}</div>}

        <div className="dialog-actions">
          <button type="button" className="btn-ghost" onClick={onClose}>
            Close
          </button>
        </div>
      </div>
    </div>
  );
}

/** Step one: the app you registered with Yahoo. */
function Credentials({
  clientId,
  clientSecret,
  redirect,
  working,
  onClientId,
  onClientSecret,
  onSave,
}: {
  clientId: string;
  clientSecret: string;
  redirect: string;
  working: boolean;
  onClientId: (value: string) => void;
  onClientSecret: (value: string) => void;
  onSave: () => void;
}) {
  return (
    <>
      <span className="mid dialog-note">
        Yahoo has no public app to borrow, so this needs one of your own. Create it at
        developer.yahoo.com/apps/create as an <strong>Installed Application</strong>, give it
        Fantasy Sports <strong>Read</strong> permission, and set the redirect URI to{" "}
        <code className="yahoo-redirect">{redirect}</code>. Yahoo then shows a client id and a
        client secret. They are kept in this Mac&apos;s keychain and never leave it.
      </span>
      <label className="field">
        Client id
        <input
          className="text-input"
          value={clientId}
          autoComplete="off"
          onChange={(e) => onClientId(e.target.value)}
        />
      </label>
      <label className="field">
        Client secret
        <input
          className="text-input"
          type="password"
          value={clientSecret}
          autoComplete="off"
          onChange={(e) => onClientSecret(e.target.value)}
        />
      </label>
      <button
        type="button"
        className="btn-primary yahoo-action"
        disabled={working || clientId.trim() === "" || clientSecret.trim() === ""}
        onClick={onSave}
      >
        {working ? "Saving…" : "Save credentials"}
      </button>
    </>
  );
}

/** Step two: the browser round trip that gets a token. */
function Connect({
  start,
  code,
  copied,
  working,
  onBegin,
  onCode,
  onCopy,
  onFinish,
}: {
  start: { authorize_url: string; state: string } | null;
  code: string;
  copied: boolean;
  working: boolean;
  onBegin: () => void;
  onCode: (value: string) => void;
  onCopy: () => void;
  onFinish: () => void;
}) {
  return (
    <>
      <span className="mid dialog-note">
        Your app is saved. Signing in opens Yahoo in your browser; approve the app there and Yahoo
        shows a short code to paste back here.
      </span>
      <button
        type="button"
        className="btn-primary yahoo-action"
        disabled={working}
        onClick={onBegin}
      >
        {working && start === null
          ? "Opening Yahoo…"
          : start === null
            ? "Sign in to Yahoo"
            : "Start again"}
      </button>
      {start !== null && (
        <>
          <span className="mid dialog-note">
            If the browser did not open, paste this address into it yourself.
          </span>
          <div className="yahoo-url">
            <span className="ellipsis yahoo-url-text">{start.authorize_url}</span>
            <button type="button" className="btn-ghost yahoo-copy" onClick={onCopy}>
              {copied ? "Copied" : "Copy"}
            </button>
          </div>
          <label className="field">
            Code from Yahoo
            <input
              className="text-input"
              value={code}
              autoComplete="off"
              onChange={(e) => onCode(e.target.value)}
              onKeyDown={(e) => {
                if (e.key === "Enter" && code.trim() !== "" && !working) onFinish();
              }}
            />
          </label>
          <button
            type="button"
            className="btn-primary yahoo-action"
            disabled={working || code.trim() === ""}
            onClick={onFinish}
          >
            {working ? "Finishing…" : "Finish"}
          </button>
        </>
      )}
    </>
  );
}

/** Step three: whose account it is, and what it plays in. */
function Connected({
  account,
  leagues,
  activeId,
  busy,
  onFind,
  onSwitch,
  onDisconnect,
}: {
  account: string | null;
  leagues: StoredLeague[] | null;
  activeId: string | null;
  busy: boolean;
  onFind: () => void;
  onSwitch: (leagueId: string) => void;
  onDisconnect: () => void;
}) {
  return (
    <>
      <span className="mid dialog-note">
        Connected as <strong>{account ?? "your Yahoo account"}</strong>. This is a read-only
        connection: the app reads leagues, rosters and draft picks, and never writes anything back.
        Disconnecting signs out of this Yahoo account and keeps the app you registered, so
        connecting again is one click.
      </span>
      {leagues !== null &&
        (leagues.length === 0 ? (
          <span className="mid dialog-note">
            That account plays in no fantasy football leagues this season.
          </span>
        ) : (
          <div className="league-list">
            {leagues.map((league) => (
              <LeagueRow
                key={league.league_id}
                league={league}
                activeId={activeId}
                busy={busy}
                onPick={() => onSwitch(league.league_id)}
              />
            ))}
          </div>
        ))}
      <div className="yahoo-buttons">
        <button type="button" className="btn-primary" disabled={busy} onClick={onFind}>
          {leagues === null ? "Find my Yahoo leagues" : "Look again"}
        </button>
        <button type="button" className="btn-ghost" disabled={busy} onClick={onDisconnect}>
          Disconnect
        </button>
      </div>
    </>
  );
}
