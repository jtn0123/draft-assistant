import { Suspense, useCallback, useEffect, useRef, useState } from "react";
import { getVersion } from "@tauri-apps/api/app";
import { api } from "./api";
import { setAvatarMode, useAvatarMode } from "./avatars";
import { playChime } from "./chime";
import { MAX_RECONNECT_ATTEMPTS, useDraftSession } from "./draftSession";
import { setChime, setScreen, useChime, useScreen } from "./prefs";
import { importSecondOpinion } from "./secondOpinionImport";
import { clearFollow, readFollow, useCompanionEnabled } from "./companion";
import { REVOKED_KEY } from "./apiRemote";
import { buildSettingsRows } from "./settingsRows";
import { useSeasonSession } from "./session";
import type { SeasonView } from "./season-types";
import type { DraftView } from "./types";
import { Header } from "./components/Header";
import { Chat, DraftScreen, ScreenFallback, SeasonScreen } from "./components/lazyScreens";
import { LaunchScreen, Setup } from "./components/Panels";
import { LeaguePicker } from "./components/LeaguePicker";
import { YahooConnect } from "./components/YahooConnect";
import { ConfirmDialog, Toast } from "./components/Overlays";
import { CompanionPanel } from "./components/CompanionPanel";
import { JoinHost } from "./components/JoinHost";
import { ErrorBoundary } from "./components/ErrorBoundary";
import { ordinal, pickLabel, problem, scoringFormat } from "./format";
import { cycleThemePreference, useAppliedTheme } from "./theme";
import { useYahooStatus } from "./yahoo";
// Only the sheets the shell itself paints with. The screen-specific ones are
// imported by the screens, so Vite ships each alongside the chunk that needs
// it rather than making every window parse all ten before first paint.
import "./theme.css";
import "./App.css";
import "./header.css";
import "./bits.css";
import "./components.css";
import "./zoom.css";
import "./yahoo.css";

type Confirm = { playerId: string; name: string } | null;

/** What the browser preview shows: it has no Tauri shell to ask, so this is
 *  kept in step with package.json and tauri.conf.json by hand. */
const PREVIEW_VERSION = "0.2.0";

/** The running app's version, from the shell that knows it. */
function useAppVersion(): string {
  const [version, setVersion] = useState(PREVIEW_VERSION);
  useEffect(() => {
    let cancelled = false;
    // Wrapped rather than called straight: outside Tauri this throws as it is
    // called, not as it settles, and the preview must simply keep the
    // fallback rather than take an unhandled rejection.
    void (async () => {
      try {
        const running = await getVersion();
        if (!cancelled) setVersion(running);
      } catch {
        // Not in the shell; PREVIEW_VERSION stands.
      }
    })();
    return () => {
      cancelled = true;
    };
  }, []);
  return version;
}

/** The pending "put this toast away" timer. Module scope rather than a ref:
 *  `showToast` is handed to the settings rows, and a function that reads a
 *  ref cannot be passed anywhere during render. One window, one toast strip,
 *  one timer — and the unmount effect still clears it. */
let toastTimer: number | undefined;

/** The note a revoked follower left for itself, taken and cleared. Null in
 *  every ordinary launch, which is all but one of them. */
function revokedNote(): { text: string } | null {
  try {
    if (localStorage.getItem(REVOKED_KEY) === null) return null;
    localStorage.removeItem(REVOKED_KEY);
  } catch {
    return null;
  }
  return { text: "The host revoked this device" };
}

/** A line under the header. `retry` marks it as something gone wrong that the
 * user can have another go at — and that should wait for them. */
type ToastMessage = { text: string; retry?: () => void };

export default function App() {
  // Remembered between sessions, along with the rest of the preferences.
  const screen = useScreen();
  const avatars = useAvatarMode();
  const [confirm, setConfirm] = useState<Confirm>(null);
  // Read once, as the window opens: `api` chose its backend off the same
  // record, so a change to it mid-session would leave the two disagreeing.
  const [follow] = useState(readFollow);
  // A revoked follower's note, read as the state is initialised so the shell
  // paints with the explanation rather than after it.
  const [toast, setToast] = useState<ToastMessage | null>(revokedNote);
  // Stable across renders so the memoised board rows are not invalidated by a
  // fresh closure on every 3-second poll. A follower has nothing to record —
  // the host keeps the picks — so it is told who does rather than being shown
  // a dialog whose only outcome is a refusal.
  const askToDraft = useCallback(
    (playerId: string, name: string) => {
      if (follow !== null) setToast({ text: `${follow.host_name} records the picks` });
      else setConfirm({ playerId, name });
    },
    [follow],
  );
  const [leaguePicker, setLeaguePicker] = useState(false);
  const [companionOpen, setCompanionOpen] = useState(false);
  const [joinOpen, setJoinOpen] = useState(false);
  const [yahooOpen, setYahooOpen] = useState(false);
  const [settingsOpen, setSettingsOpen] = useState(false);
  const [chatOpen, setChatOpen] = useState(false);
  // Reads the stored choice, keeps the page painted in it, and follows the OS
  // while the choice is "system".
  const { preference, theme } = useAppliedTheme();
  const wasMyPick = useRef(false);

  // ---------- toasts ----------

  const showToast = useCallback((text: string, retry?: () => void) => {
    setToast({ text, retry });
    window.clearTimeout(toastTimer);
    // News gets out of the way on its own. Something that failed waits to be
    // answered — a lost pick in the middle of a draft is the worst thing this
    // app could shrug off.
    if (retry === undefined) {
      toastTimer = window.setTimeout(() => setToast(null), 5000);
    }
  }, []);

  useEffect(() => () => window.clearTimeout(toastTimer), []);

  // The draft's own data lifecycle: restores the last league on launch, keeps
  // it live off the backend's poller, and owns every action that replaces the
  // whole view.
  const {
    view,
    applyView,
    polling,
    pollHealth,
    busy,
    leagues,
    restoring,
    launchError,
    attempt,
    showSetup,
    setShowSetup,
    retry,
    startLive,
    togglePolling,
    undoLastPick,
    refreshPicks,
    pullingPicks,
    exportState,
    switchLeague,
    forgetLeague,
    refreshLeagues,
    hasAccount,
    refreshData,
  } = useDraftSession(showToast);

  // The season screen's own data lifecycle: loads on first open, polls while
  // it is showing, and knows how to retry itself.
  const {
    season,
    error: seasonError,
    pollHealth: seasonPollHealth,
    retry: retrySeason,
  } = useSeasonSession(screen === "season", view?.league.league_id ?? null, showToast);
  const chime = useChime();
  // Asked once for the settings row and the picker's Yahoo lookup; the
  // connect dialog hands back every newer answer it is given.
  const yahoo = useYahooStatus();
  const appVersion = useAppVersion();
  // Only the host has a server to ask about, and the answer is re-read as the
  // dialog closes so the row never contradicts what was just switched.
  const companionOn = useCompanionEnabled(follow === null, companionOpen);

  // Chime when the clock reaches you — the one moment worth interrupting for.
  useEffect(() => {
    const isMine = view?.draft.is_my_pick ?? false;
    if (isMine && !wasMyPick.current && chime) {
      playChime();
    }
    wasMyPick.current = isMine;
  }, [view?.draft.is_my_pick, chime, view]);

  // ---------- actions ----------

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

  // ---------- screens without a league ----------

  // A league has been loaded: leave the setup screen behind and go live on it.
  const enterLeague = (loaded: DraftView) => {
    applyView(loaded);
    setShowSetup(false);
    void refreshLeagues();
    void startLive();
  };

  // The Yahoo dialog hands back an id, not a view, so this is the half the
  // Sleeper form does for itself.
  const loadLeague = async (leagueId: string) => {
    try {
      enterLeague(await api.addLeague(leagueId));
    } catch (e) {
      showToast(problem("Could not load that league", e), () => void loadLeague(leagueId));
    }
  };

  if (showSetup) {
    return (
      <div className="app">
        <Setup
          onReady={enterLeague}
          onConnectYahoo={() => setYahooOpen(true)}
          onJoinHost={() => setJoinOpen(true)}
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
        {joinOpen && <JoinHost onClose={() => setJoinOpen(false)} />}
        {yahooOpen && (
          // The same dialog the settings menu opens, which already knows how
          // to log in and list the account's leagues. Nothing here is on
          // screen yet, so the league it picks is loaded rather than switched.
          <YahooConnect
            activeId={null}
            busy={busy}
            onSwitch={(id) => {
              setYahooOpen(false);
              void loadLeague(id);
            }}
            onStatus={yahoo.setStatus}
            onClose={() => setYahooOpen(false)}
          />
        )}
      </div>
    );
  }

  if (view === null) {
    return (
      <div className="app">
        <LaunchScreen
          leagueName={restoring === null || restoring.name === "" ? null : restoring.name}
          leagueId={restoring?.league_id ?? null}
          platform={restoring?.platform ?? "sleeper"}
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

  const settingsRows = buildSettingsRows({
    view,
    chime,
    polling,
    lastSyncAt: pollHealth?.last_success_at ?? null,
    leagueCount: leagues.length,
    yahoo: yahoo.status,
    yahooConnected: yahoo.connected,
    busy,
    avatars,
    preference,
    theme,
    appVersion,
    hostName: follow?.host_name ?? null,
    companionOn,
    onChime: (next) => setChime(next),
    onTogglePolling: () => void togglePolling(),
    onLeaguePicker: () => {
      setSettingsOpen(false);
      setLeaguePicker(true);
    },
    onYahoo: () => {
      setSettingsOpen(false);
      setYahooOpen(true);
    },
    onRefreshData: () => {
      setSettingsOpen(false);
      void refreshData(() => {
        // The season view is built on the old projections until it reloads.
        if (screen === "season") retrySeason();
      });
    },
    onExport: () => {
      setSettingsOpen(false);
      void exportState();
    },
    onImportCsv: () => {
      setSettingsOpen(false);
      void importSecondOpinion(applyView, showToast);
    },
    onAvatars: setAvatarMode,
    onAppearance: cycleThemePreference,
    onCompanion: () => {
      setSettingsOpen(false);
      setCompanionOpen(true);
    },
    onJoinHost: () => {
      setSettingsOpen(false);
      setJoinOpen(true);
    },
    onLeaveHost: () => {
      // Every screen is built against the host's data, so going home is a
      // reload rather than a state change — the same way joining was.
      clearFollow();
      window.location.reload();
    },
    onDismiss: () => setSettingsOpen(false),
  });

  return (
    <div className="app">
      <div className={chatOpen ? "shell has-chat" : "shell"}>
        <div className="shell-main">
          <Header
            leagueName={view.league.name}
            hostedBy={follow?.host_name ?? null}
            onSwitchLeague={() => {
              // The host picks the league; a follower's copy of the picker
              // could only fail, so it says who to ask instead.
              if (follow !== null) {
                showToast(`${follow.host_name} picks the league`);
                return;
              }
              setLeaguePicker(true);
            }}
            subtitle={subtitle}
            meta={`${d.teams}-team ${scoringFormat(view.league.scoring_settings.rec)} · ${d.rounds} rounds${d.manual_picks_active ? " · manual picks active" : ""}`}
            screen={screen}
            onScreen={setScreen}
            polling={polling}
            pollHealth={pollHealth}
            onRefreshPicks={() => void refreshPicks()}
            refreshingPicks={pullingPicks}
            onUndo={() => void undoLastPick()}
            chatOpen={chatOpen}
            onToggleChat={() => setChatOpen((c) => !c)}
            settingsOpen={settingsOpen}
            onToggleSettings={() => setSettingsOpen((s) => !s)}
            settingsRows={settingsRows}
            footerNote={
              // Yahoo's terms ask for the attribution wherever their data is
              // shown; this is the line every screen carries under the menu.
              view.league.platform === "yahoo"
                ? "Fantasy data provided by Yahoo Fantasy · read-only connection"
                : `${view.league.name} · league ${view.league.league_id} · read-only connection`
            }
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
                // Keyed by screen and league: each keeps its own saved chats,
                // and the panel reads which one to reopen as it mounts. A
                // question about one board is not context for another.
                key={`${screen}.${view.league.league_id}`}
                screen={screen}
                leagueId={view.league.league_id}
                sharedOnly={follow !== null}
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
          hasAccount={hasAccount}
          yahooConnected={yahoo.connected}
          busy={busy}
          onSwitch={(id) => {
            setLeaguePicker(false);
            void switchLeague(id);
          }}
          onForget={(id) => void forgetLeague(id)}
          onClose={() => setLeaguePicker(false)}
        />
      )}

      {yahooOpen && (
        <YahooConnect
          activeId={view.league.league_id}
          busy={busy}
          onSwitch={(id) => {
            setYahooOpen(false);
            void switchLeague(id);
          }}
          onStatus={yahoo.setStatus}
          onClose={() => setYahooOpen(false)}
        />
      )}

      {companionOpen && <CompanionPanel onClose={() => setCompanionOpen(false)} />}

      {joinOpen && <JoinHost onClose={() => setJoinOpen(false)} />}

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
