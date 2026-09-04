// The draft screen's data lifecycle: restore the last league on launch, keep
// it live off the backend's poller, and carry out the things that replace the
// whole view — recording a pick's worth of state, switching leagues, or
// rebuilding the board from fresh projections.
//
// This lives outside App.tsx for the same reason session.ts does: the state
// machine here — restoring, connected, failed, retrying, switching — is worth
// exercising on its own rather than only through the whole rendered app, and
// App is left holding the screen rather than the connection.

import { useCallback, useEffect, useRef, useState } from "react";
import { api } from "./api";
import { stableAvailable } from "./boardIdentity";
import { platformOf } from "./leagues";
import { problem } from "./format";
import type { DraftView, PollHealth, StoredLeague } from "./types";

/** How many times the launch screen counts a failed reconnection. */
export const MAX_RECONNECT_ATTEMPTS = 4;

export interface DraftSession {
  /** The current draft view, or null until the first one has arrived. */
  view: DraftView | null;
  /** Take a freshly returned view as the current one. */
  applyView: (next: DraftView) => void;
  /** Whether the backend is polling Sleeper. */
  polling: boolean;
  /** How the backend poller's last attempt went, or null before the first. */
  pollHealth: PollHealth | null;
  /** Something is in flight that the whole screen waits for. */
  busy: boolean;
  /** Every league this app has loaded before. */
  leagues: StoredLeague[];
  /** A Sleeper account is saved, so the picker can look its leagues up. */
  hasAccount: boolean;
  /** The saved league being restored, named on the launch screen. */
  restoring: StoredLeague | null;
  /** Why the last restore failed, or null. */
  launchError: string | null;
  /** Which reconnection attempt the launch screen is showing. */
  attempt: number;
  /** No league to restore, or the user asked for a different one. */
  showSetup: boolean;
  setShowSetup: (show: boolean) => void;
  /** Try the restore again after a failure. */
  retry: () => void;
  /** Turn the backend's Sleeper poller on. False when it would not start —
   *  the toast has already said so, and nothing else should claim success. */
  startLive: () => Promise<boolean>;
  togglePolling: () => Promise<void>;
  /** Take back the last manually recorded pick. */
  undoLastPick: () => Promise<void>;
  /** Write the whole view out as JSON, and say where it went. */
  exportState: () => Promise<void>;
  switchLeague: (leagueId: string) => Promise<void>;
  /** Drop a league from the picker's list. Nothing on disk goes with it. */
  forgetLeague: (leagueId: string) => Promise<void>;
  /** Re-read the config after something else changed it (the setup screen). */
  refreshLeagues: () => Promise<void>;
  /**
   * Re-fetch projections and rebuild the board.
   *
   * @param onBoardRebuilt runs once the new view is in, for anything else
   *   built on the old projections
   */
  refreshData: (onBoardRebuilt: () => void) => Promise<void>;
}

/**
 * @param showToast how to tell the user something happened, or failed in a
 *   way they can have another go at
 */
export function useDraftSession(
  showToast: (text: string, retry?: () => void) => void,
): DraftSession {
  const [view, setView] = useState<DraftView | null>(null);
  const [polling, setPolling] = useState(false);
  const [pollHealth, setPollHealth] = useState<PollHealth | null>(null);
  const [busy, setBusy] = useState(true);
  /// Bumped to re-run the restore effect after a failed connection.
  const [reloadToken, setReloadToken] = useState(0);
  const [launchError, setLaunchError] = useState<string | null>(null);
  const [attempt, setAttempt] = useState(1);
  /// The saved league being restored, named on the launch screen.
  const [restoring, setRestoring] = useState<StoredLeague | null>(null);
  const [showSetup, setShowSetup] = useState(false);
  // Every league the app has loaded before, so switching to a mock draft and
  // back does not mean going to find an ID again.
  const [leagues, setLeagues] = useState<StoredLeague[]>([]);
  const [hasAccount, setHasAccount] = useState(false);

  // Held in a ref so a caller that hands us a fresh closure on every render
  // cannot, by itself, re-run the restore below — `startLive` is one of its
  // dependencies, and a new one every render would reconnect on every render.
  const showToastRef = useRef(showToast);
  useEffect(() => {
    showToastRef.current = showToast;
  }, [showToast]);

  const applyView = useCallback((next: DraftView) => {
    // Hand the board back the array it has already sorted when the new view
    // says exactly the same thing about the players (see boardIdentity.ts).
    // Most updates move a clock or a status, not the several-hundred-row pool.
    setView((prev) => stableAvailable(prev, next));
    setPollHealth({
      last_success_at: next.data_health.poll_last_success_at ?? next.generated_at,
      // Both are always present: the backend types them u32 and Option<String>.
      consecutive_failures: next.data_health.poll_consecutive_failures,
      last_error: next.data_health.poll_last_error,
    });
  }, []);

  // Named so the retry it offers can be itself. It answers whether polling is
  // actually running, because a caller that goes on to say "live sync on" over
  // the failure toast is telling the user the opposite of what happened.
  const startLive = useCallback(async function start(): Promise<boolean> {
    try {
      await api.startPolling(3);
      setPolling(true);
      return true;
    } catch (e) {
      showToastRef.current(problem("Could not turn live sync on", e), () => void start());
      return false;
    }
  }, []);

  // Restore the last league on mount, and again whenever the user retries.
  // State is set from the promise callbacks, never synchronously in the body.
  useEffect(() => {
    let cancelled = false;
    api
      .getConfig()
      .then((config) => {
        if (cancelled) return null;
        setLeagues(config.leagues);
        setHasAccount(config.my_user_id !== null);
        const leagueId = config.active_league_id;
        if (leagueId === null) {
          setShowSetup(true);
          return null;
        }
        setRestoring(
          config.leagues.find((l) => l.league_id === leagueId) ?? {
            league_id: leagueId,
            name: "",
            season: "",
            status: null,
            // Nothing but the id is known yet; its shape says which service
            // it belongs to.
            platform: platformOf(leagueId),
          },
        );
        return api.addLeague(leagueId);
      })
      .then((restored) => {
        if (cancelled || restored === null) return;
        applyView(restored);
        return startLive();
      })
      .catch((e) => {
        if (cancelled) return;
        setLaunchError(String(e));
        setAttempt((n) => Math.min(n + 1, MAX_RECONNECT_ATTEMPTS));
      })
      .finally(() => {
        if (!cancelled) setBusy(false);
      });
    return () => {
      cancelled = true;
    };
  }, [applyView, startLive, reloadToken]);

  const retry = () => {
    setBusy(true);
    setLaunchError(null);
    setReloadToken((n) => n + 1);
  };

  useEffect(() => {
    const un = api.onDraftUpdated(applyView);
    return () => {
      un.then((f) => f()).catch(() => undefined);
    };
  }, [applyView]);

  useEffect(() => {
    const un = api.onPollHealth(setPollHealth);
    return () => {
      un.then((f) => f()).catch(() => undefined);
    };
  }, []);

  const togglePolling = async () => {
    try {
      if (polling) {
        await api.stopPolling();
        setPolling(false);
      } else if (await startLive()) {
        showToast("Live sync on — polling Sleeper every 3s");
      }
    } catch (e) {
      showToast(problem("Could not change live sync", e), () => void togglePolling());
    }
  };

  const undoLastPick = async () => {
    try {
      applyView(await api.undoManualPick());
      showToast("Last recorded pick undone");
    } catch (e) {
      showToast(problem("Could not undo the last recorded pick", e), () => void undoLastPick());
    }
  };

  const exportState = async () => {
    try {
      showToast(`State exported: ${await api.exportState()}`);
    } catch (e) {
      showToast(problem("Could not export the state", e), () => void exportState());
    }
  };

  // Switching leagues rebuilds everything: the board comes from the new
  // league's own scoring, the season is dropped by the backend and reloaded by
  // `useSeasonSession` when it sees a different league id, and the draft
  // poller is stopped before the switch so it cannot write the old league's
  // picks over the new view on its way out.
  const switchLeague = async (leagueId: string) => {
    setBusy(true);
    try {
      if (polling) {
        await api.stopPolling();
        setPolling(false);
      }
      const next = await api.addLeague(leagueId);
      applyView(next);
      const config = await api.getConfig();
      setLeagues(config.leagues);
      setHasAccount(config.my_user_id !== null);
      // The league did switch either way; only the live-sync half may have
      // failed, and that failure has already had its own toast with a retry.
      if (await startLive()) {
        showToast(`Switched to ${next.league.name} — the last league is still in the list`);
      }
    } catch (e) {
      showToast(problem("Could not switch leagues", e), () => void switchLeague(leagueId));
    } finally {
      setBusy(false);
    }
  };

  const forgetLeague = async (leagueId: string) => {
    try {
      setLeagues(await api.removeLeague(leagueId));
    } catch (e) {
      showToast(problem("Could not forget that league", e));
    }
  };

  const refreshLeagues = async () => {
    try {
      const config = await api.getConfig();
      setLeagues(config.leagues);
      setHasAccount(config.my_user_id !== null);
    } catch (e) {
      showToast(problem("Could not re-read the league list", e));
    }
  };

  const refreshData = async (onBoardRebuilt: () => void) => {
    setBusy(true);
    try {
      const refreshed = await api.refreshData();
      applyView(refreshed);
      // The board was rebuilt from new projections, so anything else built on
      // the old ones is stale until it reloads too.
      onBoardRebuilt();
      showToast(
        `Projections refreshed — board rebuilt from ${refreshed.data_health.board_size} players`,
      );
    } catch (e) {
      showToast(
        problem("Could not refresh the projections", e),
        () => void refreshData(onBoardRebuilt),
      );
    } finally {
      setBusy(false);
    }
  };

  return {
    view,
    applyView,
    polling,
    pollHealth,
    busy,
    leagues,
    hasAccount,
    restoring,
    launchError,
    attempt,
    showSetup,
    setShowSetup,
    retry,
    startLive,
    togglePolling,
    undoLastPick,
    exportState,
    switchLeague,
    forgetLeague,
    refreshLeagues,
    refreshData,
  };
}
