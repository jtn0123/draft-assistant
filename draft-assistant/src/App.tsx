import { useCallback, useEffect, useRef, useState } from "react";
import { api } from "./api";
import type { DraftView, PollHealth, StoredLeague } from "./types";
import { errorMessage } from "./format";
import { Chat } from "./components/Chat";
import { ConfirmDialog } from "./components/ConfirmDialog";
import { Setup } from "./components/Panels";
import { DraftScreen } from "./components/DraftScreen";
import { SeasonScreen } from "./components/SeasonScreen";
import { LeaguePicker } from "./components/LeaguePicker";
import { formatAge, syncClass, syncLabel } from "./components/syncStatus";
import { useOnClockAlert } from "./components/useOnClockAlert";
import { loadAlertPref, saveAlertPref } from "./components/alertPref";
import { PickStyleContext, formatPick, loadPickStyle, savePickStyle } from "./pickFormat";
import { useViewMode } from "./viewMode";
import { subtitle } from "./subtitle";
import { LAUNCH_RETRY_DELAYS_MS, startingLaunch, transientNetworkError, withRetry } from "./launch";
import type { LaunchStatus } from "./launch";
import { LaunchScreen } from "./components/LaunchScreen";
import "./App.css";
import "./components.css";
import "./snake.css";
import "./week.css";
import "./season.css";
import "./chat.css";

// ---------- app ----------

type Confirm = { playerId: string; name: string } | null;

// Confirmations auto-dismiss as a corner toast that never intercepts a click.
// Failures and cancelled picks stay until dismissed, as a bar in the page flow
// under the header: a floating one covered the header buttons the moment they
// wrapped onto a second row.
type Toast = { text: string; sticky: boolean } | null;

export default function App() {
  const [view, setView] = useState<DraftView | null>(null);
  const [polling, setPolling] = useState(false);
  const [pollHealth, setPollHealth] = useState<PollHealth | null>(null);
  const [confirm, setConfirmState] = useState<Confirm>(null);
  const [toast, setToast] = useState<Toast>(null);
  const [busy, setBusy] = useState(true);
  // Re-read while polling so "Last sync" counts up between updates instead of
  // freezing at whatever it said when the last one arrived.
  const [now, setNow] = useState(() => Date.now());
  const [chatOpen, setChatOpen] = useState(false);
  const [alertOn, setAlertOn] = useState(loadAlertPref);
  // Overall pick number, or round.pick the way drafters say it out loud.
  const [pickStyle, setPickStyle] = useState(loadPickStyle);
  // Every league loaded so far, so switching to a mock and back is two clicks.
  const [leagues, setLeagues] = useState<StoredLeague[]>([]);
  // Restoring the saved league at launch, attempt by attempt, so the screen
  // can say "still trying" and, in the end, "unable to connect" with a way
  // back — rather than a blank wait and then an empty setup form.
  const [launch, setLaunch] = useState<LaunchStatus | null>(null);
  const [setupWanted, setSetupWanted] = useState(false);
  const [activeLeagueId, setActiveLeagueId] = useState<string | null>(null);
  // The draft cockpit or the season screen: the season once the draft is over,
  // either on request, remembered per draft.
  const [mode, setMode] = useViewMode(view?.draft.draft_id ?? null, view?.draft.status ?? null);
  const toastTimer = useRef<number | undefined>(undefined);
  // Highest view seq already rendered. The 3s poll and the awaited click
  // handlers both push views with no ordering guarantee, so without this a
  // poll that started earlier can land later and overwrite a fresher result.
  const appliedSeq = useRef(0);
  // Mirrors `confirm` so the update callback below can read the pending pick
  // without re-subscribing to the poller on every open/close.
  const confirmRef = useRef<Confirm>(null);
  // React's dev-mode double mount ran the launch effect twice, and two full
  // league loads then raced on the same cache files. Load once per mount.
  const booted = useRef(false);

  const setConfirm = useCallback((next: Confirm) => {
    confirmRef.current = next;
    setConfirmState(next);
  }, []);

  const notify = useCallback((text: string) => {
    window.clearTimeout(toastTimer.current);
    setToast({ text, sticky: false });
    toastTimer.current = window.setTimeout(() => setToast(null), 4000);
  }, []);

  const fail = useCallback((text: string) => {
    window.clearTimeout(toastTimer.current);
    setToast({ text, sticky: true });
  }, []);

  const dismissToast = () => {
    window.clearTimeout(toastTimer.current);
    setToast(null);
  };

  const applyView = useCallback((next: DraftView) => {
    if (next.seq <= appliedSeq.current) return;
    appliedSeq.current = next.seq;
    setView(next);
    // The confirm modal holds a click-time snapshot. Live sync can draft that
    // player to someone else while it sits open, and confirming would then
    // record a pick that never happened.
    const pending = confirmRef.current;
    if (
      pending &&
      !next.available.some((p) => p.player_id === pending.playerId)
    ) {
      setConfirm(null);
      fail(`${pending.name} was drafted by another team — pick cancelled`);
    }
    setPollHealth({
      last_success_at:
        next.data_health.poll_last_success_at ?? next.generated_at,
      consecutive_failures:
        next.data_health.poll_consecutive_failures ?? 0,
      last_error: next.data_health.poll_last_error ?? null,
    });
  }, [setConfirm, fail]);

  // Returns whether sync actually started: the caller must not announce
  // success on a failed start, which is how "Live sync on" once came to
  // overwrite the very error explaining that it was off.
  const startLive = useCallback(async (): Promise<boolean> => {
    try {
      await api.startPolling(3);
      setPolling(true);
      return true;
    } catch (e) {
      fail(errorMessage(e));
      return false;
    }
  }, [fail]);

  // Restore a league by id: the saved one at launch, or again from the
  // launch screen. A stalled connect is tried again; the screen follows.
  const restore = useCallback(
    async (id: string) => {
      setLaunch(startingLaunch(id));
      setSetupWanted(false);
      setBusy(true);
      try {
        const v = await withRetry(
          () => api.addLeague(id),
          LAUNCH_RETRY_DELAYS_MS,
          transientNetworkError,
          (attempt, error) => setLaunch((l) => l && { ...l, attempt, error }),
        );
        setLaunch(null);
        applyView(v);
        if (!api.preview || api.replay) await startLive();
      } catch (e) {
        setLaunch((l) => l && { ...l, failed: true, error: errorMessage(e) });
      } finally {
        setBusy(false);
      }
    },
    [applyView, startLive],
  );

  // Restore last league on launch; live sync starts automatically (except in
  // the browser preview, where there is nothing to sync).
  useEffect(() => {
    if (booted.current) return;
    booted.current = true;
    (async () => {
      try {
        const config = await api.getConfig();
        setLeagues(config.leagues);
        setActiveLeagueId(config.active_league_id);
        if (config.active_league_id) await restore(config.active_league_id);
      } catch (e) {
        fail(errorMessage(e));
      } finally {
        setBusy(false);
      }
    })();
  }, [fail, restore]);

  // Live updates from the poller. A rejected payload is surfaced rather than
  // dropped: updates silently stopping is the one failure this app must never
  // have while the pill still says "Live sync on".
  useEffect(() => {
    const un = api.onDraftUpdated(applyView, (e) =>
      fail(`Live update rejected: ${errorMessage(e)}`),
    );
    return () => {
      un.then((f) => f()).catch(() => undefined);
    };
  }, [applyView, fail]);

  useEffect(() => {
    if (!polling) return;
    const timer = window.setInterval(() => setNow(Date.now()), TICK_MS);
    return () => window.clearInterval(timer);
  }, [polling]);

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
        notify("Live sync on — polling Sleeper every 3s");
      }
    } catch (e) {
      fail(errorMessage(e));
    }
  };

  const doDraft = async (playerId: string) => {
    try {
      const v = await api.recordManualPick(playerId);
      // Clear first so the stale-pick effect above cannot fire on our own pick.
      setConfirm(null);
      applyView(v);
    } catch (e) {
      fail(errorMessage(e));
      setConfirm(null);
    }
  };

  const doUndo = async () => {
    try {
      applyView(await api.undoManualPick());
    } catch (e) {
      fail(errorMessage(e));
    }
  };

  const doExport = async () => {
    try {
      const path = await api.exportState();
      notify(`State exported: ${path}`);
    } catch (e) {
      fail(errorMessage(e));
    }
  };

  /// Load another league (or a bare draft ID) and make it the active one.
  useOnClockAlert({
    onClock: view?.draft.is_my_pick === true && view.draft.status === "drafting",
    currentPick: view?.draft.current_pick,
    pickLabel:
      view === null ? undefined : formatPick(view.draft.current_pick, view.draft.teams, pickStyle),
    enabled: alertOn,
  });

  const doSwitchLeague = async (leagueId: string) => {
    setBusy(true);
    try {
      applyView(await api.addLeague(leagueId));
      const config = await api.getConfig();
      setLeagues(config.leagues);
      setActiveLeagueId(config.active_league_id);
      if (!api.preview) await startLive();
      notify("Loaded — the previous league is still in the list");
    } catch (e) {
      fail(errorMessage(e));
    } finally {
      setBusy(false);
    }
  };

  const doRefreshData = async () => {
    setBusy(true);
    try {
      applyView(await api.refreshData());
      notify(
        api.replay
          ? "Replay state reloaded — projections are only refetched in the desktop app"
          : "Projections refreshed and board rebuilt",
      );
    } catch (e) {
      fail(errorMessage(e));
    } finally {
      setBusy(false);
    }
  };

  // A launch failure must be visible on the screen the user actually lands on.
  // The bar used to live only on the main screen, so a failed restore showed
  // Setup with no hint that anything had gone wrong.
  const alertBar = toast?.sticky ? (
    <div className="alert-bar" role="alert">
      <span>{toast.text}</span>
      <button className="ghost small" onClick={dismissToast} aria-label="Dismiss message">
        ✕
      </button>
    </div>
  ) : null;

  if (view === null) {
    const wantsSetup = setupWanted || (!busy && launch === null);
    return !wantsSetup ? (
      <LaunchScreen
        status={launch}
        onRetry={() => launch && void restore(launch.leagueId)}
        onSetup={() => setSetupWanted(true)}
      />
    ) : (
      <>
        {alertBar}
        <Setup
          initialLeagueId={activeLeagueId ?? undefined}
          onReady={(v) => {
            applyView(v);
            if (!api.preview || api.replay) void startLive();
          }}
        />
      </>
    );
  }

  const pickLabel = (pick: number) => formatPick(pick, view.draft.teams, pickStyle);

  // The chat is a column beside the page, not an overlay: the numbers being
  // asked about stay visible next to the answer.
  return (
    <PickStyleContext.Provider value={pickStyle}>
    <div className={chatOpen ? "shell with-chat" : "shell"}>
    <div className="app">
      {api.preview && (
        <div className="notice" role="note">
          {api.replay
            ? `Browser preview — replaying ${api.replay}, read-only. Run the desktop app to draft.`
            : "Browser preview — fixture data, read-only. Run the desktop app to draft."}
        </div>
      )}
      <header>
        <div className="brand">
          <h1>{view.league.name}</h1>
          <span className="muted">
            {subtitle(view, mode)}
          </span>
          <div className="mode-switch" role="group" aria-label="Screen">
            <button
              aria-label="Draft screen"
              aria-pressed={mode === "draft"}
              onClick={() => setMode("draft")}
            >
              Draft
            </button>
            <button
              aria-label="Season screen"
              aria-pressed={mode === "season"}
              onClick={() => setMode("season")}
            >
              Season
            </button>
          </div>
          {!api.preview && (
            <LeaguePicker
              leagues={leagues}
              activeId={activeLeagueId}
              disabled={busy}
              onSwitch={(id) => void doSwitchLeague(id)}
            />
          )}
        </div>
        <div className="actions">
          <button
            className={`live ${syncClass(polling, pollHealth, now)}`}
            onClick={togglePolling}
            title={pollHealth?.last_error ?? undefined}
          >
            {syncLabel(polling, pollHealth, now)}
          </button>
          {polling && pollHealth?.last_success_at && (
            <span className="sync-age">
              Last sync {formatAge(pollHealth.last_success_at, now)} ago
            </span>
          )}
          {mode === "draft" && (
            <>
          <button
            className="ghost"
            onClick={doUndo}
            disabled={!view.draft.manual_picks_active}
            title={
              view.draft.manual_picks_active
                ? "Undo last manual pick"
                : "No manual picks to undo"
            }
          >
            Undo
          </button>
          <button
            className={`ghost ${alertOn ? "on" : ""}`}
            onClick={() => {
              const next = !alertOn;
              setAlertOn(next);
              saveAlertPref(next);
              notify(next ? "Chime on when you're on the clock" : "Chime off");
            }}
            title={
              alertOn
                ? "Chime and flash the window title when your pick comes up"
                : "Alerts off — the title still changes"
            }
            aria-label={alertOn ? "Turn on-clock chime off" : "Turn on-clock chime on"}
          >
            {alertOn ? "🔔" : "🔕"}
          </button>
          <button
            className={`ghost ${pickStyle === "round" ? "on" : ""}`}
            onClick={() => {
              const next = pickStyle === "round" ? "overall" : "round";
              setPickStyle(next);
              savePickStyle(next);
              notify(
                next === "round"
                  ? "Picks shown as round.pick"
                  : "Picks shown as the overall number",
              );
            }}
            title={
              pickStyle === "round"
                ? "Showing round.pick — switch to overall pick numbers"
                : "Showing overall pick numbers — switch to round.pick"
            }
            aria-label="Toggle pick numbering"
          >
            {/* Labelled with the live pick in the current notation, so the
                button shows what it does rather than describing it. */}
            {pickStyle === "round" ? formatPick(view.draft.current_pick, view.draft.teams, "round") : `#${view.draft.current_pick}`}
          </button>
            </>
          )}
          <button
            className={`ghost ${chatOpen ? "on" : ""}`}
            onClick={() => setChatOpen((v) => !v)}
            title={mode === "season" ? "Ask Claude about the week" : "Ask Claude about the current draft"}
          >
            Ask Claude
          </button>
          <button className="ghost" onClick={doExport} title="Write full draft state JSON for the AI">
            Export state
          </button>
          <button className="ghost" onClick={doRefreshData} disabled={busy} title="Re-fetch projections and rebuild the board">
            {busy ? "Refreshing…" : "Refresh data"}
          </button>
        </div>
      </header>

      {alertBar}

      {mode === "season" ? (
        <SeasonScreen view={view} />
      ) : (
        <DraftScreen view={view} onDraft={(id, name) => setConfirm({ playerId: id, name })} />
      )}

      {confirm && (
        <ConfirmDialog
          name={confirm.name}
          pick={pickLabel(view.draft.current_pick)}
          slot={view.draft.on_clock_slot}
          onConfirm={() => void doDraft(confirm.playerId)}
          onCancel={() => setConfirm(null)}
        />
      )}

      {toast && !toast.sticky && (
        <div className="toast" role="status">
          <span>{toast.text}</span>
        </div>
      )}
    </div>
    <Chat
      open={chatOpen}
      onClose={() => setChatOpen(false)}
      currentPick={view.draft.current_pick}
      draftId={view.draft.draft_id}
      leagueName={view.league.name}
      onClock={view.draft.is_my_pick && view.draft.status === "drafting"}
      onAutoAsk={() => setChatOpen(true)}
      seasonMode={view.draft.status === "complete"}
    />
    </div>
    </PickStyleContext.Provider>
  );
}

const TICK_MS = 5000;
