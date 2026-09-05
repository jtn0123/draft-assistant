// The one sound the app makes, and the rule about when it is allowed to.

import { useEffect, useRef } from "react";
import { playChime } from "./chime";
import type { DraftView } from "./types";

/**
 * Chime once when the clock reaches you.
 *
 * A paused draft is the exception. Sleeper keeps reporting `is_my_pick` while
 * the commissioner has the draft stopped, so a room that paused for twenty
 * minutes had the app chiming at whoever was next as if it were their turn,
 * over and over as views came and went. Nobody is on the clock while it is
 * paused, so nothing is worth interrupting for.
 *
 * @param enabled the user's chime preference
 */
export function usePickChime(view: DraftView | null, enabled: boolean): void {
  const wasMine = useRef(false);
  useEffect(() => {
    const mine = view !== null && view.draft.is_my_pick && !view.draft.paused;
    if (mine && !wasMine.current && enabled) playChime();
    wasMine.current = mine;
  }, [view, enabled]);
}
