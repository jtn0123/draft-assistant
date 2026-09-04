// One league, as a row you can click.
//
// It started life inside the picker and moved out when the Yahoo dialog
// wanted to list leagues too. Two lists of leagues that look different from
// each other would be a small lie about them being the same kind of thing, so
// there is one row and both dialogs use it.

import type { RefObject } from "react";
import { leagueNote, platformMark } from "../leagues";
import type { StoredLeague } from "../types";

export function LeagueRow({
  league,
  activeId,
  busy,
  buttonRef,
  onPick,
  onForget,
}: {
  league: StoredLeague;
  /** The league on screen right now; this row is marked when it matches. */
  activeId: string | null;
  /** True while something the row would start is already in flight. */
  busy: boolean;
  /** Set on the row the dialog wants to open with the focus on. */
  buttonRef?: RefObject<HTMLButtonElement | null>;
  onPick: () => void;
  /** Given only for a league that can be dropped from the list. */
  onForget?: () => void;
}) {
  // A league Sleeper or Yahoo has never been asked to name is still worth
  // offering; its id is the only thing there is to call it.
  const name = league.name === "" ? league.league_id : league.name;
  const active = league.league_id === activeId;
  const mark = platformMark(league.platform);
  return (
    <div className="league-row">
      <button
        type="button"
        className={active ? "league-option is-on" : "league-option"}
        aria-pressed={active}
        disabled={busy}
        ref={buttonRef}
        onClick={onPick}
      >
        <span className="league-option-head">
          <span className="league-option-name ellipsis">{name}</span>
          {mark !== null && <span className="platform-pill">{mark}</span>}
        </span>
        <span className="muted league-option-note">{leagueNote(league, activeId)}</span>
      </button>
      {onForget !== undefined && (
        <button
          type="button"
          className="league-forget"
          aria-label={`Forget ${name}`}
          title="Drop from this list. Its cached data stays."
          disabled={busy}
          onClick={onForget}
        >
          Forget
        </button>
      )}
    </div>
  );
}
