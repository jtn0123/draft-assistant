// Whether Yahoo is connected, asked once and remembered.
//
// Two things need the answer and neither should ask for it: the settings row,
// which says "Not connected" or names the account, and the league picker,
// which only looks up Yahoo leagues when there is a token to look them up
// with. So the shell holds one copy, the connect dialog hands back every
// status the backend gave it, and nothing asks twice.
//
// A failure here is not worth a toast. Not knowing means not connected, which
// is what the settings row would say anyway, and the connect dialog asks
// again — and reports properly — the moment it opens.

import { useEffect, useState } from "react";
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

export function useYahooStatus(): YahooConnection {
  const [status, setStatus] = useState<YahooStatus | null>(null);

  // Nothing is set in the effect body: the answer lands from the promise.
  useEffect(() => {
    let cancelled = false;
    api
      .yahooStatus()
      .then((next) => {
        if (!cancelled) setStatus(next);
      })
      .catch(() => undefined);
    return () => {
      cancelled = true;
    };
  }, []);

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
