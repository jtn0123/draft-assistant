// Switch to another league — one this app has loaded before, one Sleeper says
// the account plays in, or a brand-new league or mock draft pasted by ID.
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
  busy,
  onSwitch,
  onClose,
}: {
  /** Leagues already known from the config. */
  leagues: StoredLeague[];
  activeId: string | null;
  /** The season to look up on Sleeper — the one the app has data for. */
  season: string;
  /** True while a switch is in flight; the whole dialog waits for it. */
  busy: boolean;
  onSwitch: (leagueId: string) => void;
  onClose: () => void;
}) {
  const dialog = useRef<HTMLDivElement>(null);
  const firstOption = useRef<HTMLButtonElement>(null);
  const opener = useRef<HTMLElement | null>(null);
  const [found, setFound] = useState<StoredLeague[]>([]);
  const [looking, setLooking] = useState(false);
  const [error, setError] = useState<string | null>(null);
  const [pasted, setPasted] = useState("");

  useEffect(() => {
    opener.current = document.activeElement as HTMLElement | null;
    firstOption.current?.focus();
    return () => {
      opener.current?.focus();
      opener.current = null;
    };
  }, []);

  useFocusTrap(dialog, onClose);

  const look = async () => {
    setLooking(true);
    setError(null);
    try {
      setFound(await api.sleeperLeagues(season));
    } catch (e) {
      setError(String(e).replace(/^Error:\s*/, ""));
    } finally {
      setLooking(false);
    }
  };

  const options = mergeLeagues(leagues, found, activeId);

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
            {options.map((league, index) => (
              <button
                key={league.league_id}
                type="button"
                className={league.league_id === activeId ? "league-option is-on" : "league-option"}
                aria-pressed={league.league_id === activeId}
                disabled={busy}
                ref={index === 0 ? firstOption : undefined}
                onClick={() =>
                  league.league_id === activeId ? onClose() : onSwitch(league.league_id)
                }
              >
                <span className="league-option-name ellipsis">
                  {league.name === "" ? league.league_id : league.name}
                </span>
                <span className="muted league-option-note">{leagueNote(league, activeId)}</span>
              </button>
            ))}
          </div>
        )}

        <button
          type="button"
          className="btn-ghost league-find"
          disabled={busy || looking}
          onClick={() => void look()}
        >
          {looking ? "Asking Sleeper…" : "Find my leagues on Sleeper"}
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
