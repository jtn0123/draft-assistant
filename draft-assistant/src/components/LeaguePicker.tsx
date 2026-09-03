// Switch to another league — one this app has loaded before, one Sleeper says
// the account plays in, or a brand-new league or mock draft pasted by ID.
//
// When an account is saved, Sleeper is asked the moment the dialog opens, so
// a league joined since the last visit is already in the list; the button
// stays for asking again. A league the app has loaded can be forgotten from
// its row, which trims the list and nothing else.
//
// It is a dialog rather than a control in the header because switching is the
// most disruptive thing the shell can do: it tears the board down, drops the
// season, and restarts both pollers. Local's other "are you sure" moment (the
// manual pick) is a dialog too, and this reuses its scrim, its focus trap and
// its buttons.

import { useEffect, useRef, useState } from "react";
import { api } from "../api";
import { leagueNote, mergeLeagues } from "../leagues";
import type { StoredLeague } from "../types";
import { useFocusTrap } from "./useFocusTrap";

export function LeaguePicker({
  leagues,
  activeId,
  season,
  hasAccount,
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
  const [asking, setAsking] = useState(false);
  const [error, setError] = useState<string | null>(null);
  const [pasted, setPasted] = useState("");
  const [asked, setAsked] = useState(false);

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
        if (!cancelled) setError(String(e).replace(/^Error:\s*/, ""));
      });
    return () => {
      cancelled = true;
    };
  }, [hasAccount, season]);

  const look = async () => {
    setAsking(true);
    setError(null);
    try {
      setFound(await api.sleeperLeagues(season));
      setAsked(true);
    } catch (e) {
      setError(String(e).replace(/^Error:\s*/, ""));
    } finally {
      setAsking(false);
    }
  };

  const looking = asking || (hasAccount && !asked && error === null);

  const options = mergeLeagues(leagues, found, activeId);
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
              const name = league.name === "" ? league.league_id : league.name;
              const active = league.league_id === activeId;
              return (
                <div key={league.league_id} className="league-row">
                  <button
                    type="button"
                    className={active ? "league-option is-on" : "league-option"}
                    aria-pressed={active}
                    disabled={busy}
                    ref={index === 0 ? firstOption : undefined}
                    onClick={() => (active ? onClose() : onSwitch(league.league_id))}
                  >
                    <span className="league-option-name ellipsis">{name}</span>
                    <span className="muted league-option-note">{leagueNote(league, activeId)}</span>
                  </button>
                  {!active && stored.has(league.league_id) && (
                    <button
                      type="button"
                      className="league-forget"
                      aria-label={`Forget ${name}`}
                      title="Drop from this list. Its cached data stays."
                      disabled={busy}
                      onClick={() => onForget(league.league_id)}
                    >
                      Forget
                    </button>
                  )}
                </div>
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

        <label className="field">
          Or paste a league or draft
          <input
            className="text-input"
            value={pasted}
            placeholder="1389710366300200960 or a sleeper.com link"
            onChange={(e) => setPasted(e.target.value)}
            onKeyDown={(e) => {
              if (e.key === "Enter" && pasted.trim() !== "" && !busy) onSwitch(pasted.trim());
            }}
          />
        </label>

        {error !== null && <div className="error">{error}</div>}

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
