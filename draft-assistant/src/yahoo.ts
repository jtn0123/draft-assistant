// Whether Yahoo is connected, asked when it could have changed and remembered.
//
// Two things need the answer and neither should ask for it: the settings row,
// which says "Not connected" or names the account, and the league picker,
// which only looks up Yahoo leagues when there is a token to look them up
// with. So the shell holds one copy, the connect dialog hands back every
// status the backend gave it, and nothing asks twice for the same answer.
//
// It is asked again at the two moments the dialog's own answers cannot cover.
// When the dialog closes: a loopback sign-in finishes in the backend after
// the browser comes back, with nothing in the dialog to hand a status up, so
// the row said "Not connected" over an account that was connected. And when
// the poller says Yahoo signed the user out: the backend clears the dead pair
// mid-draft and no dialog is open to notice, so the row went on saying
// "Connected as ..." until the app was restarted.
//
// A failure here is not worth a toast. Not knowing means not connected, which
// is what the settings row would say anyway, and the connect dialog asks
// again, and reports properly, the moment it opens.

import { useCallback, useEffect, useRef, useState } from "react";
import { api } from "./api";
import type { YahooStatus } from "./types";

export interface YahooConnection {
  /** The last status the backend gave, or null before the first answer. */
  status: YahooStatus | null;
  /** True only once a token is in hand. */
  connected: boolean;
  /** Take a status the connect dialog was handed. */
  setStatus: (status: YahooStatus) => void;
}

/** The sentence the backend's poller reports when Yahoo refused the stored
 *  pair (`yahoo::SIGNED_OUT`). Matched loosely: the tick joins its reasons
 *  with other errors, and the row only needs to know to look again. */
export function saysSignedOut(pollError: string | null): boolean {
  return pollError !== null && /signed you out/i.test(pollError);
}

/**
 * @param dialogOpen whether the Connect Yahoo dialog is showing; the status is
 *   asked again as it closes.
 * @param pollError the draft poller's last error, so a sign-out the backend
 *   found mid-draft reaches the row without a dialog in between.
 */
export function useYahooStatus(
  dialogOpen = false,
  pollError: string | null = null,
): YahooConnection {
  const [status, setStatus] = useState<YahooStatus | null>(null);
  // Which ask is the latest. An older answer landing after a newer one would
  // put a stale status back on the row.
  const asked = useRef(0);

  const ask = useCallback(() => {
    asked.current += 1;
    const generation = asked.current;
    api
      .yahooStatus()
      .then((next) => {
        if (asked.current === generation) setStatus(next);
      })
      .catch(() => undefined);
  }, []);

  // Nothing is set in the effect body: the answer lands from the promise.
  useEffect(ask, [ask]);

  // Closing, not opening: the dialog asks for itself as it opens and hands the
  // answer up, so asking then would be the second of two identical calls.
  const wasOpen = useRef(false);
  useEffect(() => {
    if (wasOpen.current && !dialogOpen) ask();
    wasOpen.current = dialogOpen;
  }, [dialogOpen, ask]);

  useEffect(() => {
    if (saysSignedOut(pollError)) ask();
  }, [pollError, ask]);

  return { status, connected: status?.connected ?? false, setStatus };
}

/** What the settings row says under "Yahoo".
 *
 *  Not knowing yet reads as not connected, which is the honest answer: it is
 *  what the row would say if the lookup had come back and said so, and the
 *  dialog behind it asks again and reports properly either way. */
export function yahooNote(status: YahooStatus | null): string {
  if (status?.connected !== true) return "Not connected";
  return `Connected as ${status.account ?? "your Yahoo account"}`;
}
