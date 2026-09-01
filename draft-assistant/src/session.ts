// The season screen's data lifecycle: load it once when the tab is first
// opened, keep it live while it is showing, and expose a retry for when the
// load fails.
//
// This lives outside App.tsx so the state machine — loading, loaded, failed,
// retrying — can be exercised on its own rather than only through the whole
// rendered app.

import { useCallback, useEffect, useRef, useState } from "react";
import { api } from "./api";
import type { SeasonView } from "./season-types";
import type { PollHealth } from "./types";

/** How often to poll live scoring while the season screen is open, seconds. */
const LIVE_INTERVAL = 30;

export interface SeasonSession {
  /** The current view, or null while it is loading or has failed. */
  season: SeasonView | null;
  /** Why the last load failed, or null. */
  error: string | null;
  /**
   * How the live-scoring poller's last attempt went, or null before the first
   * one has finished. This is the only place a poll failure shows up: a failed
   * refresh leaves the view exactly as it was, so without this the screen has
   * nothing to notice.
   */
  pollHealth: PollHealth | null;
  /** Re-fetch from Sleeper, bypassing the cache. */
  retry: () => void;
}

/**
 * @param active whether the season screen is showing
 * @param leagueId the loaded league, or null before there is one — nothing
 *   can be fetched until there is, and a change of it means everything held
 *   here belongs to a league the user has switched away from
 * @param onError called with the message when a load fails, for the toast
 */
export function useSeasonSession(
  active: boolean,
  leagueId: string | null,
  onError: (message: string) => void,
): SeasonSession {
  const ready = leagueId !== null;
  const [season, setSeason] = useState<SeasonView | null>(null);
  const [error, setError] = useState<string | null>(null);
  const [pollHealth, setPollHealth] = useState<PollHealth | null>(null);

  // Held in a ref so a caller that hands us a fresh closure on every render
  // cannot, by itself, re-run the load or restart the poller.
  const onErrorRef = useRef(onError);
  useEffect(() => {
    onErrorRef.current = onError;
  }, [onError]);

  // Whether the one-off load has already been kicked off. A ref rather than a
  // read of `season`, because every pushed live update replaces `season` — and
  // if that were an input to the effects below, each update would tear the
  // whole lifecycle down and build it again.
  const loadedRef = useRef(false);

  // Switching leagues invalidates everything above: the standings, the week,
  // and the poller's own idea of what it is polling. Declared before the load
  // and poll effects so that when the league changes, this has already put
  // them back to "nothing loaded yet" by the time they run.
  const firstLeague = useRef(leagueId);
  useEffect(() => {
    if (firstLeague.current === leagueId) return;
    firstLeague.current = leagueId;
    loadedRef.current = false;
    setSeason(null);
    setPollHealth(null);
    setError(null);
  }, [leagueId]);

  // Live updates are pushed from the backend whenever the score moves.
  useEffect(() => {
    const un = api.onSeasonUpdated(setSeason);
    return () => {
      un.then((f) => f()).catch(() => undefined);
    };
  }, []);

  // The other half of the same feed: every poll reports whether it got
  // through, including the ones that brought no new scores because they
  // failed. Kept separate from `season` so a run of failures does not disturb
  // the last good view — the numbers stay, labelled as not moving.
  useEffect(() => {
    const un = api.onSeasonPollHealth(setPollHealth);
    return () => {
      un.then((f) => f()).catch(() => undefined);
    };
  }, []);

  // The first load: once, the first time the screen is showing and a league
  // is ready.
  useEffect(() => {
    if (!active || !ready || loadedRef.current) return undefined;
    loadedRef.current = true;
    let live = true;
    api
      .loadSeason(false)
      .then(setSeason)
      .catch((e) => {
        // A failed first load must not lock the screen out of ever loading;
        // opening it again is allowed to try once more.
        loadedRef.current = false;
        if (!live) return;
        setError(String(e));
        onErrorRef.current(String(e));
      });
    return () => {
      live = false;
    };
  }, [active, ready, leagueId]);

  // Polling runs for exactly as long as the screen is showing. Nothing else
  // is allowed to restart it, so the backend's own thirty-second timer gets to
  // keep its schedule instead of being cancelled and recreated on every tick.
  useEffect(() => {
    if (!active || !ready) return undefined;
    api.startSeasonPolling(LIVE_INTERVAL).catch((e) => {
      // A poller that never started looks exactly like a screen that quietly
      // stopped moving, so say it out loud rather than leaving the numbers to
      // go stale in silence.
      onErrorRef.current(`Live updates are not running: ${String(e)}`);
    });
    return () => {
      // Stop polling as soon as the screen is not showing: nothing renders it.
      // A failure here is not worth a message — the screen is on its way out,
      // and the next time it opens it starts a fresh poller anyway.
      api.stopSeasonPolling().catch(() => undefined);
    };
  }, [active, ready, leagueId]);

  const retry = useCallback(() => {
    setError(null);
    api
      .loadSeason(true)
      .then((next) => {
        loadedRef.current = true;
        setSeason(next);
      })
      .catch((e) => setError(String(e)));
  }, []);

  return { season, error, pollHealth, retry };
}

/** Force a fresh season load, for the Settings "Refresh data" path. */
export async function reloadSeason(): Promise<SeasonView> {
  return api.loadSeason(true);
}
