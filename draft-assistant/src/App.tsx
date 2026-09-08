import { Suspense, useState } from "react";
import { api } from "./api";
import { footerNote, headerMeta, headerSubtitle } from "./appSubtitle";
import { useUpdateRow } from "./useUpdateRow";
import { setAvatarMode, useAvatarMode } from "./avatars";
import { MAX_RECONNECT_ATTEMPTS, useDraftEnd, useDraftSession } from "./draftSession";
import { useMarkDrafted } from "./markDrafted";
import { usePickChime } from "./pickChime";
import type { Screen } from "./prefs";
import { setAskButton, setChime, setScreen, useAskButton, useChime, useScreen } from "./prefs";
import { importSecondOpinion } from "./secondOpinionImport";
import { clearFollow, readFollow, useCompanionEnabled, useFollowStatus } from "./companion";
import { useHostSync } from "./hostSync";
import { SkipLink } from "./components/SkipLink";
import { useToast } from "./toast";
import { buildSettingsRows, headerMenuRows } from "./settingsRows";
import { useSeasonSession } from "./session";
import type { DraftView } from "./types";
import { SettingsPage } from "./components/SettingsPage";
import { SleeperIdentityPicker } from "./components/SleeperIdentityPicker";
import { Header } from "./components/Header";
import {
  Chat,
  DraftScreen,
  ProjectionsScreen,
  ScreenFallback,
  SeasonScreen,
} from "./components/lazyScreens";
import { LaunchScreen, Setup } from "./components/Panels";
import { HostWaiting } from "./components/SetupScreens";
import { LeaguePicker } from "./components/LeaguePicker";
import { YahooConnect } from "./components/YahooConnect";
import { ConfirmDialog, ToastStrip } from "./components/Overlays";
import { CompanionPanel } from "./components/CompanionPanel";
import { Diagnostics } from "./components/Diagnostics";
import { JoinHost } from "./components/JoinHost";
import { ErrorBoundary } from "./components/ErrorBoundary";
import { pickLabel, problem } from "./format";
import { cycleThemePreference, useAppliedTheme } from "./theme";
import { useYahooStatus } from "./yahoo";
import "./theme.css";
import "./App.css";
import "./header.css";
import "./bits.css";
import "./components.css";
import "./zoom.css";
import "./yahoo.css";

export default function App() {
  const savedScreen = useScreen();
  const avatars = useAvatarMode();
  const [joined] = useState(readFollow);
  const hostSync = useHostSync();
  const follow =
    joined === null ? null : { ...joined, host_name: hostSync.hostName ?? joined.host_name };
  const { toast, showToast, dismissToast } = useToast();
  const followStatus = useFollowStatus();
  const [leaguePicker, setLeaguePicker] = useState(false);
  const [companionOpen, setCompanionOpen] = useState(false);
  const [joinOpen, setJoinOpen] = useState(false);
  const [diagnosticsOpen, setDiagnosticsOpen] = useState(false);
  const [yahooOpen, setYahooOpen] = useState(false);
  const [settingsOpen, setSettingsOpen] = useState(false);
  const [settingsPageOpen, setSettingsPageOpen] = useState(false);
  const closeSettings = () => {
    setSettingsOpen(false);
    setSettingsPageOpen(false);
  };
  const [chatOpen, setChatOpen] = useState(false);
  const { preference, theme } = useAppliedTheme();
  const [setupFromLaunch, setSetupFromLaunch] = useState(false);

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

  const screen: Screen = view?.league.platform === "yahoo" ? "draft" : savedScreen;

  const {
    season,
    error: seasonError,
    pollHealth: seasonPollHealth,
    retry: retrySeason,
  } = useSeasonSession(screen !== "draft", view?.league.league_id ?? null, showToast);
  const chime = useChime();
  const askButton = useAskButton();
  const {
    confirm,
    ask: askToDraft,
    cancel: cancelDraft,
    confirmDraft,
    drafting,
  } = useMarkDrafted(
    applyView,
    showToast,
    follow?.host_name ?? null,
    view?.league.league_id ?? null,
  );
  const yahoo = useYahooStatus(yahooOpen, pollHealth?.last_error ?? null);
  const updates = useUpdateRow();
  const companionOn = useCompanionEnabled(follow === null, companionOpen);

  usePickChime(view, chime);

  const { onDraft, onUndo } = useDraftEnd(view, askToDraft, undoLastPick, showToast);
  const clearKeepers = async () => {
    try {
      applyView(await api.clearKeepers());
    } catch (e) {
      showToast(problem("Could not clear the keepers", e), () => void clearKeepers());
    }
  };

  // ---------- screens without a league ----------

  // A league has been loaded: leave the setup screen behind and go live on it.
  const enterLeague = (loaded: DraftView) => {
    applyView(loaded);
    setSetupFromLaunch(false);
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

  // Every screen is built against the host's data, so going home is a reload
  // rather than a state change — the same way joining was. The settings row
  // and the "Pair again" the header offers a revoked follower are one path.
  const leaveHost = () => {
    clearFollow();
    window.location.reload();
  };

  // The league the user actually came from: the one on screen when Settings
  // opened this form, else the saved one being restored. `restoring` alone
  // named the launch-time league after a switch.
  const cameFrom = view?.league.name ?? restoring?.name ?? "";

  if (showSetup) {
    // A follower's league is whatever the host has open: the Sleeper form,
    // Yahoo and Join would all be refused, so it gets a wait and a way out.
    if (follow !== null) {
      return (
        <div className="app">
          <HostWaiting
            hostName={follow.host_name}
            onRetry={() => {
              setShowSetup(false);
              retry();
            }}
            onLeaveHost={leaveHost}
          />
        </div>
      );
    }
    return (
      <div className="app">
        {setupFromLaunch && (
          // Asking for a different league is not a decision you are stuck with:
          // this screen had no way out at all, so a mis-click on the launch
          // screen meant quitting the app to get the saved league back.
          <button
            type="button"
            className="link-btn"
            onClick={() => {
              setSetupFromLaunch(false);
              setShowSetup(false);
            }}
          >
            {cameFrom === "" ? "Back" : `Back to ${cameFrom}`}
          </button>
        )}
        <Setup
          activeLeagueId={view?.league.league_id ?? null}
          onReady={enterLeague}
          onConnectYahoo={() => setYahooOpen(true)}
          onJoinHost={() => setJoinOpen(true)}
        />
        <ToastStrip toast={toast} onDismiss={dismissToast} />
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
          onDifferentLeague={() => {
            setSetupFromLaunch(true);
            setShowSetup(true);
          }}
          hostName={follow?.host_name ?? null}
          onLeaveHost={follow === null ? undefined : leaveHost}
        />
      </div>
    );
  }

  // ---------- the app ----------

  const d = view.draft;
  const subtitle = headerSubtitle(screen, view, season);

  const settingsRows = buildSettingsRows({
    view,
    chime,
    ask: askButton,
    polling,
    lastSyncAt: pollHealth?.last_success_at ?? null,
    leagueCount: leagues.length,
    yahoo: yahoo.status,
    yahooConnected: yahoo.connected,
    busy,
    avatars,
    preference,
    theme,
    updates,
    hostName: follow?.host_name ?? null,
    companionOn,
    onChime: (next) => setChime(next),
    onAsk: (next) => setAskButton(next),
    onTogglePolling: () => void togglePolling(),
    onLeaguePicker: () => {
      closeSettings();
      setLeaguePicker(true);
    },
    onYahoo: () => {
      closeSettings();
      setYahooOpen(true);
    },
    onRefreshData: () => {
      closeSettings();
      void refreshData(() => {
        // The season view is built on the old projections until it reloads.
        if (screen !== "draft") retrySeason();
      });
    },
    onExport: () => {
      closeSettings();
      void exportState();
    },
    onImportCsv: () => {
      closeSettings();
      void importSecondOpinion(applyView, showToast);
    },
    onClearKeepers: () => {
      closeSettings();
      void clearKeepers();
    },
    onSetUsername: () => {
      setSettingsOpen(false);
      setSettingsPageOpen(true);
      requestAnimationFrame(() =>
        document.querySelector<HTMLInputElement>("#sleeper-identity-picker input")?.focus(),
      );
    },
    onAvatars: setAvatarMode,
    onAppearance: cycleThemePreference,
    onCompanion: () => {
      closeSettings();
      setCompanionOpen(true);
    },
    onJoinHost: () => {
      closeSettings();
      setJoinOpen(true);
    },
    onLeaveHost: leaveHost,
    onDiagnostics: () => {
      closeSettings();
      setDiagnosticsOpen(true);
    },
    onDismiss: closeSettings,
  });

  return (
    <div className="app">
      <SkipLink target="board-main">Skip to the board</SkipLink>
      <div className={chatOpen ? "shell has-chat" : "shell"}>
        <div className="shell-main">
          <Header
            leagueName={view.league.name}
            hostedBy={follow?.host_name ?? null}
            followStatus={follow === null ? null : followStatus}
            onPairAgain={leaveHost}
            onSwitchLeague={() => {
              if (follow !== null) {
                showToast(`${follow.host_name} picks the league`);
                return;
              }
              setLeaguePicker(true);
            }}
            subtitle={subtitle}
            meta={headerMeta(view)}
            screen={screen}
            onScreen={setScreen}
            platform={view.league.platform}
            polling={follow === null ? polling : hostSync.polling}
            pollHealth={pollHealth}
            onRefreshPicks={() => void refreshPicks()}
            refreshingPicks={pullingPicks}
            onUndo={onUndo}
            chatOpen={chatOpen}
            onToggleChat={() => setChatOpen((c) => !c)}
            settingsOpen={settingsOpen}
            onToggleSettings={() => setSettingsOpen((s) => !s)}
            settingsRows={headerMenuRows(settingsRows, () => {
              setSettingsOpen(false);
              setSettingsPageOpen(true);
            })}
            footerNote={footerNote(view)}
          />

          <ToastStrip toast={toast} onDismiss={dismissToast} />

          {/* The one landmark past the header, and where the skip link lands:
              without it a screen reader had the whole app under `banner` and
              no way to jump to the board. `tabIndex` so the link can move the
              keyboard here as well as the reading position. */}
          <main id="board-main" tabIndex={-1}>
            {/* Keyed per screen: one reused instance kept the season screen's
              crash on screen after switching to the draft, and back. */}
            {screen === "draft" ? (
              <ErrorBoundary key="draft">
                <Suspense fallback={<ScreenFallback />}>
                  <DraftScreen
                    view={view}
                    busy={busy}
                    onDraft={onDraft}
                    readOnly={follow !== null}
                  />
                </Suspense>
              </ErrorBoundary>
            ) : screen === "projections" ? (
              <ErrorBoundary key="projections">
                <Suspense fallback={<ScreenFallback />}>
                  <ProjectionsScreen draft={view} season={season} />
                </Suspense>
              </ErrorBoundary>
            ) : season !== null ? (
              <ErrorBoundary key="season">
                <Suspense fallback={<ScreenFallback />}>
                  <SeasonScreen view={season} pollHealth={seasonPollHealth} />
                </Suspense>
              </ErrorBoundary>
            ) : seasonError !== null ? (
              <div className="season-loading is-error" role="alert">
                <span>{seasonError}</span>
                <button type="button" className="btn-primary" onClick={retrySeason}>
                  Try again
                </button>
              </div>
            ) : (
              <div className="season-loading">Loading this week…</div>
            )}
          </main>
        </div>

        {chatOpen && (
          <ErrorBoundary>
            <Suspense fallback={null}>
              <Chat
                key={`${screen}.${view.league.league_id}`}
                screen={screen === "draft" ? "draft" : "season"}
                leagueId={view.league.league_id}
                sharedOnly={follow !== null}
                contextNote={
                  screen !== "draft" && season !== null
                    ? `Sees week ${season.week} · your lineup and the league`
                    : `Sees this draft · pick ${pickLabel(d.current_pick, d.teams)}`
                }
                onClose={() => setChatOpen(false)}
              />
            </Suspense>
          </ErrorBoundary>
        )}
      </div>

      {settingsPageOpen && (
        <SettingsPage
          rows={settingsRows}
          leagueName={view.league.name}
          screen={screen === "draft" ? "draft" : "season"}
          onClose={closeSettings}
          identity={
            follow === null && view.league.platform === "sleeper" ? (
              <SleeperIdentityPicker view={view} onSaved={applyView} />
            ) : undefined
          }
        />
      )}

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

      {diagnosticsOpen && (
        <Diagnostics appVersion={updates.current} onClose={() => setDiagnosticsOpen(false)} />
      )}

      {confirm && (
        <ConfirmDialog
          pickLabel={`Pick ${pickLabel(d.current_pick, d.teams)} · slot ${d.on_clock_slot}`}
          playerName={confirm.name}
          platform={view.league.platform}
          busy={drafting}
          onConfirm={confirmDraft}
          onCancel={cancelDraft}
        />
      )}
    </div>
  );
}
