// The season screen's data lifecycle: load it once when the tab is first
// opened, keep it live while it is showing, and expose a retry for when the
// load fails.
//
// This lives outside App.tsx so the state machine — loading, loaded, failed,
// retrying — can be exercised on its own rather than only through the whole
// rendered app.

import { useCallback, useEffect, useState } from "react";
import { api } from "./api";
import type { SeasonView } from "./season-types";

/** How often to poll live scoring while the season screen is open, seconds. */
const LIVE_INTERVAL = 30;

export interface SeasonSession {
  /** The current view, or null while it is loading or has failed. */
  season: SeasonView | null;
  /** Why the last load failed, or null. */
  error: string | null;
  /** Re-fetch from Sleeper, bypassing the cache. */
  retry: () => void;
}

/**
 * @param active whether the season screen is showing
 * @param ready  whether a league is loaded — nothing can be fetched before it
 * @param onError called with the message when a load fails, for the toast
 */
export function useSeasonSession(
  active: boolean,
  ready: boolean,
  onError: (message: string) => void,
): SeasonSession {
  const [season, setSeason] = useState<SeasonView | null>(null);
  const [error, setError] = useState<string | null>(null);

  // Live updates are pushed from the backend whenever the score moves.
  useEffect(() => {
    const un = api.onSeasonUpdated(setSeason);
    return () => {
      un.then((f) => f()).catch(() => undefined);
    };
  }, []);

  useEffect(() => {
    if (!active || !ready) return undefined;
    let cancelled = false;
    if (season === null) {
      api
        .loadSeason(false)
        .then((next) => {
          if (!cancelled) setSeason(next);
        })
        .catch((e) => {
          if (cancelled) return;
          setError(String(e));
          onError(String(e));
        });
    }
    api.startSeasonPolling(LIVE_INTERVAL).catch(() => undefined);
    return () => {
      cancelled = true;
      // Stop polling as soon as the screen is not showing: nothing renders it.
      api.stopSeasonPolling().catch(() => undefined);
    };
  }, [active, ready, season, onError]);

  const retry = useCallback(() => {
    setError(null);
    api
      .loadSeason(true)
      .then(setSeason)
      .catch((e) => setError(String(e)));
  }, []);

  return { season, error, retry };
}

/** Force a fresh season load, for the Settings "Refresh data" path. */
export async function reloadSeason(): Promise<SeasonView> {
  return api.loadSeason(true);
}
