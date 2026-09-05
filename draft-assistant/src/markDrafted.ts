// Recording a pick by hand: the confirm dialog's state, and the one call
// behind it.
//
// Split out of App.tsx because the interesting part is not the dialog but what
// happens when the button is pressed twice, which is worth exercising on its
// own rather than only through the whole rendered shell.

import { useCallback, useRef, useState } from "react";
import { api } from "./api";
import { describeError } from "./errorText";
import { problem } from "./format";
import type { DraftView } from "./types";

/** Which player the dialog is asking about, or null when it is closed. */
export interface DraftConfirm {
  playerId: string;
  name: string;
}

export interface MarkDrafted {
  confirm: DraftConfirm | null;
  /** Open the dialog for a player, or tell a follower who records the picks. */
  ask: (playerId: string, name: string) => void;
  cancel: () => void;
  /** Record the player the dialog is asking about. */
  confirmDraft: () => void;
  /** A call is out. The confirm button is disabled while it is. */
  drafting: boolean;
}

/**
 * True when the backend is refusing because this player is already off the
 * board. It is the one failure that means the same thing as success.
 */
function alreadyDrafted(e: unknown): boolean {
  return /already drafted|already been drafted|already picked/i.test(describeError(e));
}

/**
 * @param hostName set on a follower, which has no picks of its own to record
 *   and is told who does rather than shown a dialog that could only refuse
 */
export function useMarkDrafted(
  applyView: (next: DraftView) => void,
  showToast: (text: string, retry?: () => void) => void,
  hostName: string | null,
): MarkDrafted {
  const [confirm, setConfirm] = useState<DraftConfirm | null>(null);
  const [drafting, setDrafting] = useState(false);
  // The guard the button cannot give us: React batches state, so two clicks
  // inside one frame both read `drafting` as false and both send the pick.
  const inFlight = useRef(false);
  // Players this window has already recorded. A double tap that got two calls
  // out ends with the second one refused for a pick the first one made, which
  // is the app arguing with itself in front of the user.
  const recorded = useRef(new Set<string>());

  // Named so the retry it offers can be itself, the same way `startLive` in
  // draftSession.ts is.
  const draft = useCallback(
    async function record(playerId: string, name: string) {
      if (inFlight.current) return;
      inFlight.current = true;
      setDrafting(true);
      try {
        applyView(await api.recordManualPick(playerId));
        recorded.current.add(playerId);
      } catch (e) {
        if (!(alreadyDrafted(e) && recorded.current.has(playerId))) {
          showToast(
            problem(`Could not mark ${name} as drafted`, e),
            () => void record(playerId, name),
          );
        }
      } finally {
        inFlight.current = false;
        setDrafting(false);
        setConfirm(null);
      }
    },
    [applyView, showToast],
  );

  // Stable across renders so the memoised board rows are not invalidated by a
  // fresh closure on every 3-second poll.
  const ask = useCallback(
    (playerId: string, name: string) => {
      if (hostName !== null) showToast(`${hostName} records the picks`);
      else setConfirm({ playerId, name });
    },
    [hostName, showToast],
  );

  const cancel = useCallback(() => setConfirm(null), []);

  const confirmDraft = useCallback(() => {
    if (confirm === null) return;
    void draft(confirm.playerId, confirm.name);
  }, [confirm, draft]);

  return { confirm, ask, cancel, confirmDraft, drafting };
}
