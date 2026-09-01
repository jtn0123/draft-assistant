import { Suspense, useCallback, useEffect, useRef, useState } from "react";
import { api } from "./api";
import { setAvatarMode, useAvatarMode } from "./avatars";
import { playChime } from "./chime";
import { setChime, setScreen, useChime, useScreen } from "./prefs";
import { stableAvailable } from "./boardIdentity";
import { useSeasonSession } from "./session";
import type { DraftView, PollHealth, StoredLeague } from "./types";
import type { SeasonView } from "./season-types";
import { Header, type SettingsRow } from "./components/Header";
import { Chat, DraftScreen, ScreenFallback, SeasonScreen } from "./components/lazyScreens";
import { LaunchScreen, Setup } from "./components/Panels";
import { LeaguePicker } from "./components/LeaguePicker";
import { ConfirmDialog, Toast } from "./components/Overlays";
import { ErrorBoundary } from "./components/ErrorBoundary";
import { ordinal, pickLabel, age, problem, scoringFormat } from "./format";
import { cycleThemePreference, useAppliedTheme } from "./theme";
// Only the sheets the shell itself paints with. The screen-specific ones are
// imported by the screens, so Vite ships each alongside the chunk that needs
// it rather than making every window parse all ten before first paint.
import "./theme.css";
import "./App.css";
import "./components.css";
import "./zoom.css";

const MAX_RECONNECT_ATTEMPTS = 4;

type Confirm = { playerId: string; name: string } | null;

/** A line under the header. `retry` marks it as something gone wrong that the
 * user can have another go at — and that should wait for them. */
type ToastMessage = { text: string; retry?: () => void };

export default function App() {
  const [view, setView] = useState<DraftView | null>(null);
  // Remembered between sessions, along with the rest of the preferences.
  const screen = useScreen();
  const [polling, setPolling] = useState(false);
  const avatars = useAvatarMode();
  const [pollHealth, setPollHealth] = useState<PollHealth | null>(null);
  const [confirm, setConfirm] = useState<Confirm>(null);
  const [toast, setToast] = useState<ToastMessage | null>(null);
  const [busy, setBusy] = useState(true);
  // Stable across renders so the memoised board rows are not invalidated by a
  // fresh closure on every 3-second poll.
  const askToDraft = useCallback(
    (playerId: string, name: string) => setConfirm({ playerId, name }),
    [],
  );
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
  const [leaguePicker, setLeaguePicker] = useState(false);
  const [settingsOpen, setSettingsOpen] = useState(false);
  const [chatOpen, setChatOpen] = useState(false);
  // Reads the stored choice, keeps the page painted in it, and follows the OS
  // while the choice is "system".
  const { preference, theme } = useAppliedTheme();
  const toastTimer = useRef<number | undefined>(undefined);
  const wasMyPick = useRef(false);

  // ---------- toasts ----------

  const showToast = useCallback((text: string, retry?: () => void) => {
    setToast({ text, retry });
    window.clearTimeout(toastTimer.current);
    // News gets out of the way on its own. Something that failed waits to be
    // answered — a lost pick in the middle of a draft is the worst thing this
    // app could shrug off.
    if (retry === undefined) {
      toastTimer.current = window.setTimeout(() => setToast(null), 5000);
    }
  }, []);

  useEffect(() => () => window.clearTimeout(toastTimer.current), []);

  // The season screen's own data lifecycle: loads on first open, polls while
  // it is showing, and knows how to retry itself.
  const {
    season,
    error: seasonError,
    pollHealth: seasonPollHealth,
    retry: retrySeason,
  } = useSeasonSession(screen === "season", view?.league.league_id ?? null, showToast);
  const chime = useChime();

  // ---------- data ----------

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

  // Named so the retry it offers can be itself.
  const startLive = useCallback(
    async function start(): Promise<void> {
      try {
        await api.startPolling(3);
        setPolling(true);
      } catch (e) {
        showToast(problem("Could not turn live sync on", e), () => void start());
      }
    },
    [showToast],
  );

  // Restore the last league on mount, and again whenever the user retries.
  // State is set from the promise callbacks, never synchronously in the body.
  useEffect(() => {
    let cancelled = false;
    api
      .getConfig()
      .then((config) => {
        if (cancelled) return null;
        setLeagues(config.leagues);
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

  // Chime when the clock reaches you — the one moment worth interrupting for.
  useEffect(() => {
    const isMine = view?.draft.is_my_pick ?? false;
    if (isMine && !wasMyPick.current && chime) {
      playChime();
    }
    wasMyPick.current = isMine;
  }, [view?.draft.is_my_pick, chime, view]);

  // ---------- actions ----------

  const togglePolling = async () => {
    try {
      if (polling) {
        await api.stopPolling();
        setPolling(false);
      } else {
        await startLive();
        showToast("Live sync on — polling Sleeper every 3s");
      }
    } catch (e) {
      showToast(problem("Could not change live sync", e), () => void togglePolling());
    }
  };

  const doDraft = async (playerId: string, name: string) => {
    try {
      applyView(await api.recordManualPick(playerId));
    } catch (e) {
      showToast(
        problem(`Could not mark ${name} as drafted`, e),
        () => void doDraft(playerId, name),
      );
    } finally {
      setConfirm(null);
    }
  };

  const doUndo = async () => {
    try {
      applyView(await api.undoManualPick());
      showToast("Last recorded pick undone");
    } catch (e) {
      showToast(problem("Could not undo the last recorded pick", e), () => void doUndo());
    }
  };

  const doExport = async () => {
    setSettingsOpen(false);
    try {
      showToast(`State exported: ${await api.exportState()}`);
    } catch (e) {
      showToast(problem("Could not export the state", e), () => void doExport());
    }
  };

  // Switching leagues rebuilds everything: the board comes from the new
  // league's own scoring, the season is dropped by the backend and reloaded by
  // `useSeasonSession` when it sees a different league id, and the draft
  // poller is stopped before the switch so it cannot write the old league's
  // picks over the new view on its way out.
  const doSwitchLeague = async (leagueId: string) => {
    setLeaguePicker(false);
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
      await startLive();
      showToast(`Switched to ${next.league.name} — the last league is still in the list`);
    } catch (e) {
      showToast(problem("Could not switch leagues", e), () => void doSwitchLeague(leagueId));
    } finally {
      setBusy(false);
    }
  };

  const doRefreshData = async () => {
    setSettingsOpen(false);
    setBusy(true);
    try {
      const refreshed = await api.refreshData();
      applyView(refreshed);
      // The board was rebuilt from new projections, so the season view is
      // built on stale numbers until it reloads too.
      if (screen === "season") retrySeason();
      showToast(
        `Projections refreshed — board rebuilt from ${refreshed.data_health.board_size} players`,
      );
    } catch (e) {
      showToast(problem("Could not refresh the projections", e), () => void doRefreshData());
    } finally {
      setBusy(false);
    }
  };

  // ---------- screens without a league ----------

  if (showSetup) {
    return (
      <div className="app">
        <Setup
          onReady={(v) => {
            applyView(v);
            setShowSetup(false);
            void startLive();
          }}
        />
      </div>
    );
  }

  if (view === null) {
    return (
      <div className="app">
        <LaunchScreen
          leagueName={restoring === null || restoring.name === "" ? null : restoring.name}
          leagueId={restoring?.league_id ?? null}
          attempt={attempt}
          maxAttempts={MAX_RECONNECT_ATTEMPTS}
          lastError={busy ? null : launchError}
          onRetry={retry}
          onDifferentLeague={() => setShowSetup(true)}
        />
      </div>
    );
  }

  // ---------- the app ----------

  const d = view.draft;
  const subtitle =
    screen === "season"
      ? season === null
        ? `${view.league.season} season`
        : `Week ${season.week} · ${myRecord(season)}`
      : `Round ${d.current_round} of ${d.rounds} · ${d.total_picks_made} picks in`;

  const settingsRows: SettingsRow[] = [
    {
      label: "Pick chime",
      note: "Sound when you're on the clock",
      value: chime ? "On" : "Off",
      on: chime,
      onSelect: () => setChime(!chime),
    },
    {
      label: "Live sync",
      note: polling
        ? `Last sync ${age(pollHealth?.last_success_at ?? null)}`
        : "Not polling Sleeper",
      value: polling ? "On" : "Off",
      on: polling,
      onSelect: () => void togglePolling(),
    },
    {
      label: "League",
      note: leagues.length > 1 ? `${leagues.length} leagues loaded` : "Switch or add a league",
      value: "Switch",
      on: false,
      onSelect: () => {
        setSettingsOpen(false);
        setLeaguePicker(true);
      },
    },
    {
      label: "Refresh data",
      note: "Re-fetch projections and rebuild the board",
      value: busy ? "…" : "Sync",
      on: false,
      onSelect: () => void doRefreshData(),
    },
    {
      label: "Export state",
      note: "Full JSON dump of everything on screen",
      value: "JSON",
      on: false,
      onSelect: () => void doExport(),
    },
    {
      label: "Player pictures",
      note:
        avatars === "headshots"
          ? "Headshots from Sleeper, saved on this Mac after the first look"
          : "Team logos only — no photo downloads",
      value: avatars === "headshots" ? "Headshots" : "Team logos",
      on: avatars === "headshots",
      onSelect: () => setAvatarMode(avatars === "headshots" ? "logos" : "headshots"),
    },
    {
      label: "Appearance",
      note:
        preference === "system"
          ? "Following your system setting"
          : "Overriding your system setting",
      value: preference === "system" ? `System (${theme})` : theme === "dark" ? "Dark" : "Light",
      on: theme === "dark",
      // system -> light -> dark -> back to system
      onSelect: cycleThemePreference,
    },
  ];

  return (
    <div className="app">
      <div className={chatOpen ? "shell has-chat" : "shell"}>
        <div className="shell-main">
          <Header
            leagueName={view.league.name}
            subtitle={subtitle}
            meta={`${d.teams}-team ${scoringFormat(view.league.scoring_settings.rec)} · ${d.rounds} rounds${d.manual_picks_active ? " · manual picks active" : ""}`}
            screen={screen}
            onScreen={setScreen}
            polling={polling}
            pollHealth={pollHealth}
            onUndo={() => void doUndo()}
            chatOpen={chatOpen}
            onToggleChat={() => setChatOpen((c) => !c)}
            settingsOpen={settingsOpen}
            onToggleSettings={() => setSettingsOpen((s) => !s)}
            settingsRows={settingsRows}
            footerNote={`${view.league.name} · league ${view.league.league_id} · read-only connection`}
          />

          {toast !== null && (
            <Toast
              message={toast.text}
              action={
                toast.retry === undefined ? undefined : { label: "Try again", onClick: toast.retry }
              }
              onDismiss={() => setToast(null)}
            />
          )}

          {screen === "draft" ? (
            <ErrorBoundary>
              <Suspense fallback={<ScreenFallback />}>
                <DraftScreen view={view} busy={busy} onDraft={askToDraft} />
              </Suspense>
            </ErrorBoundary>
          ) : season !== null ? (
            <ErrorBoundary>
              <Suspense fallback={<ScreenFallback />}>
                <SeasonScreen view={season} pollHealth={seasonPollHealth} />
              </Suspense>
            </ErrorBoundary>
          ) : seasonError !== null ? (
            <div className="season-loading is-error">
              <span>{seasonError}</span>
              <button type="button" className="btn-primary" onClick={retrySeason}>
                Try again
              </button>
            </div>
          ) : (
            <div className="season-loading">Loading this week…</div>
          )}
        </div>

        {chatOpen && (
          <ErrorBoundary>
            <Suspense fallback={null}>
              <Chat
                // Keyed by screen: each screen keeps its own saved chats, and
                // the panel reads which one to reopen as it mounts.
                key={screen}
                screen={screen}
                contextNote={
                  screen === "season" && season !== null
                    ? `Sees week ${season.week} · your lineup and the league`
                    : `Sees this draft · pick ${pickLabel(d.current_pick, d.teams)}`
                }
                onClose={() => setChatOpen(false)}
              />
            </Suspense>
          </ErrorBoundary>
        )}
      </div>

      {leaguePicker && (
        <LeaguePicker
          leagues={leagues}
          activeId={view.league.league_id}
          season={view.league.season}
          busy={busy}
          onSwitch={(id) => void doSwitchLeague(id)}
          onClose={() => setLeaguePicker(false)}
        />
      )}

      {confirm && (
        <ConfirmDialog
          pickLabel={`Pick ${pickLabel(d.current_pick, d.teams)} · slot ${d.on_clock_slot}`}
          playerName={confirm.name}
          onConfirm={() => void doDraft(confirm.playerId, confirm.name)}
          onCancel={() => setConfirm(null)}
        />
      )}
    </div>
  );
}

function myRecord(season: SeasonView): string {
  const mine = season.standings.find((s) => s.is_mine);
  if (mine === undefined) return `${season.standings.length} teams`;
  return `${mine.record} · ${ordinal(mine.seed)} of ${season.standings.length}`;
}
