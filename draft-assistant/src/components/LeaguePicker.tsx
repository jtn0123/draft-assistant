// Switch to another league — one this app has loaded before, one an account
// says it plays in, or a brand-new league or mock draft pasted by ID.
//
// When an account is saved, it is asked the moment the dialog opens, so a
// league joined since the last visit is already in the list; the button stays
// for asking again. Both platforms are asked, when both are connected, and
// the two answers land in one list. A league the app has loaded can be
// forgotten from its row, which trims the list and nothing else.
//
// It is a dialog rather than a control in the header because switching is the
// most disruptive thing the shell can do: it tears the board down, drops the
// season, and restarts both pollers. Local's other "are you sure" moment (the
// manual pick) is a dialog too, and this reuses its scrim, its focus trap and
// its buttons.

import { useEffect, useRef, useState } from "react";
import { api } from "../api";
import { describeError } from "../errorText";
import { mergeLeagues } from "../leagues";
import type { StoredLeague } from "../types";
import { LeagueRow } from "./LeagueRow";
import { useFocusTrap } from "./useFocusTrap";

/** A lookup failure with the account it belongs to on the front, because
 *  "sign-in expired" under Sleeper's button was Yahoo's news half the time. */
function lookupProblem(service: string, e: unknown): string {
  return `${service}: ${describeError(e)}`;
}

export function LeaguePicker({
  leagues,
  activeId,
  season,
  hasAccount,
  yahooConnected,
  busy,
  onSwitch,
  onForget,
  onClose,
}: {
  /** Leagues already known from the config. */
  leagues: StoredLeague[];
  activeId: string | null;
  /** The season to look up on Sleeper — the one the app has data for. */
  season: string;
  /** A Sleeper account is saved, so its leagues are looked up on open. */
  hasAccount: boolean;
  /** Yahoo has a token, so its leagues are looked up on open too. */
  yahooConnected: boolean;
  /** True while a switch is in flight; the whole dialog waits for it. */
  busy: boolean;
  onSwitch: (leagueId: string) => void;
  /** Drop a league the app has loaded from the list. */
  onForget: (leagueId: string) => void;
  onClose: () => void;
}) {
  const dialog = useRef<HTMLDivElement>(null);
  const firstOption = useRef<HTMLButtonElement>(null);
  const opener = useRef<HTMLElement | null>(null);
  const [found, setFound] = useState<StoredLeague[]>([]);
  const [yahooFound, setYahooFound] = useState<StoredLeague[]>([]);
  const [asking, setAsking] = useState(false);
  const [yahooAsking, setYahooAsking] = useState(false);
  // One error slot per account. They used to share one, so a Yahoo failure
  // wrote itself under Sleeper's button, ended Sleeper's "looking" state
  // while that lookup was still out, and could not be retried on its own.
  const [error, setError] = useState<string | null>(null);
  const [yahooError, setYahooError] = useState<string | null>(null);
  const [pasted, setPasted] = useState("");
  const [asked, setAsked] = useState(false);
  const [yahooAsked, setYahooAsked] = useState(false);

  useEffect(() => {
    opener.current = document.activeElement as HTMLElement | null;
    firstOption.current?.focus();
    return () => {
      opener.current?.focus();
      opener.current = null;
    };
  }, []);

  useFocusTrap(dialog, onClose);

  // The lookup on open sets nothing synchronously: every answer lands from
  // the promise, and "looking" is read off what has and has not come back.
  useEffect(() => {
    if (!hasAccount) return;
    let cancelled = false;
    api
      .sleeperLeagues(season)
      .then((found) => {
        if (cancelled) return;
        setFound(found);
        setAsked(true);
      })
      .catch((e: unknown) => {
        if (!cancelled) setError(lookupProblem("Sleeper", e));
      });
    return () => {
      cancelled = true;
    };
  }, [hasAccount, season]);

  // Yahoo is a second account with its own list, its own button and its own
  // error. A failure here says which service failed and leaves everything the
  // picker already had on screen.
  useEffect(() => {
    if (!yahooConnected) return;
    let cancelled = false;
    api
      .yahooLeagues()
      .then((leagues) => {
        if (cancelled) return;
        setYahooFound(leagues);
        setYahooAsked(true);
      })
      .catch((e: unknown) => {
        if (!cancelled) setYahooError(lookupProblem("Yahoo", e));
      });
    return () => {
      cancelled = true;
    };
  }, [yahooConnected]);

  const look = async () => {
    setAsking(true);
    setError(null);
    try {
      setFound(await api.sleeperLeagues(season));
      setAsked(true);
    } catch (e) {
      setError(lookupProblem("Sleeper", e));
    } finally {
      setAsking(false);
    }
  };

  const lookYahoo = async () => {
    setYahooAsking(true);
    setYahooError(null);
    try {
      setYahooFound(await api.yahooLeagues());
      setYahooAsked(true);
    } catch (e) {
      setYahooError(lookupProblem("Yahoo", e));
    } finally {
      setYahooAsking(false);
    }
  };

  // Each button waits on its own account. Reading the shared error here is
  // what let a Yahoo failure put "Find my leagues on Sleeper" back on a button
  // whose lookup was still in flight, and let it be pressed a second time.
  const looking = asking || (hasAccount && !asked && error === null);
  const yahooLooking = yahooAsking || (yahooConnected && !yahooAsked && yahooError === null);

  const options = mergeLeagues(leagues, [...found, ...yahooFound], activeId);
  const stored = new Set(leagues.map((l) => l.league_id));

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
        aria-labelledby="league-picker-title"
      >
        <span className="eyebrow">League</span>
        <span className="dialog-title" id="league-picker-title">
          Switch league
        </span>
        <span className="mid dialog-note">
          Everything on screen is rebuilt for the league you pick — the board, the season, and both
          pollers. Recorded manual picks stay with the draft they were made in.
        </span>

        {options.length > 0 && (
          <div className="league-list">
            {options.map((league, index) => {
              const active = league.league_id === activeId;
              return (
                <LeagueRow
                  key={league.league_id}
                  league={league}
                  activeId={activeId}
                  busy={busy}
                  buttonRef={index === 0 ? firstOption : undefined}
                  onPick={() => (active ? onClose() : onSwitch(league.league_id))}
                  onForget={
                    !active && stored.has(league.league_id)
                      ? () => onForget(league.league_id)
                      : undefined
                  }
                />
              );
            })}
          </div>
        )}

        <button
          type="button"
          className="btn-ghost league-find"
          disabled={busy || looking}
          onClick={() => void look()}
        >
          {looking ? "Asking Sleeper…" : asked ? "Ask Sleeper again" : "Find my leagues on Sleeper"}
        </button>

        {yahooConnected && (
          <button
            type="button"
            className="btn-ghost league-find"
            disabled={busy || yahooLooking}
            onClick={() => void lookYahoo()}
          >
            {yahooLooking
              ? "Asking Yahoo…"
              : yahooAsked
                ? "Ask Yahoo again"
                : "Find my leagues on Yahoo"}
          </button>
        )}

        <label className="field">
          Or paste a league or draft
          <input
            className="text-input"
            value={pasted}
            placeholder="1389710366300200960, a sleeper.com link, or a Yahoo key like 449.l.12345"
            onChange={(e) => setPasted(e.target.value)}
            onKeyDown={(e) => {
              if (e.key === "Enter" && pasted.trim() !== "" && !busy) onSwitch(pasted.trim());
            }}
          />
        </label>

        {error !== null && <div className="error">{error}</div>}
        {yahooError !== null && <div className="error">{yahooError}</div>}

        <div className="dialog-actions">
          <button
            type="button"
            className="btn-primary"
            disabled={busy || pasted.trim() === ""}
            onClick={() => onSwitch(pasted.trim())}
          >
            {busy ? "Loading…" : "Load"}
          </button>
          <button type="button" className="btn-ghost" onClick={onClose}>
            Cancel
          </button>
        </div>
      </div>
    </div>
  );
}
