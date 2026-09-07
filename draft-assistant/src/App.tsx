import { Suspense, useState } from "react";
import { api } from "./api";
import { footerNote, headerMeta, headerSubtitle } from "./appSubtitle";
import { useUpdateRow } from "./useUpdateRow";
import { setAvatarMode, useAvatarMode } from "./avatars";
import { MAX_RECONNECT_ATTEMPTS, useDraftEnd, useDraftSession } from "./draftSession";
import { useMarkDrafted } from "./markDrafted";
import { usePickChime } from "./pickChime";
import { setChime, setScreen, useChime, useScreen, type Screen } from "./prefs";
import { importSecondOpinion } from "./secondOpinionImport";
import { clearFollow, readFollow, useCompanionEnabled, useFollowStatus } from "./companion";
import { useToast } from "./toast";
import { buildSettingsRows } from "./settingsRows";
import { useSeasonSession } from "./session";
import type { DraftView } from "./types";
import { Header } from "./components/Header";
import { Chat, DraftScreen, ScreenFallback, SeasonScreen } from "./components/lazyScreens";
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

export default function App() {
  // Remembered between sessions, along with the rest of the preferences.
  const savedScreen = useScreen();
  const avatars = useAvatarMode();
  // Read once, as the window opens: `api` chose its backend off the same
  // record, so a change to it mid-session would leave the two disagreeing.
  const [follow] = useState(readFollow);
  // One strip, one timer, and the revoked follower's note on the way in.
  const { toast, showToast, dismissToast } = useToast();
  // Whether this follower can still hear its host, for the line beside the
  // "Hosted by" pill. A host that is not following anyone has no state to show.
  const followStatus = useFollowStatus();
  const [leaguePicker, setLeaguePicker] = useState(false);
  const [companionOpen, setCompanionOpen] = useState(false);
  const [joinOpen, setJoinOpen] = useState(false);
  const [diagnosticsOpen, setDiagnosticsOpen] = useState(false);
  const [yahooOpen, setYahooOpen] = useState(false);
  const [settingsOpen, setSettingsOpen] = useState(false);
  const [chatOpen, setChatOpen] = useState(false);
  // Reads the stored choice, keeps the page painted in it, and follows the OS
  // while the choice is "system".
  const { preference, theme } = useAppliedTheme();
  // True when the setup screen was reached from the launch screen rather than
  // because there is no league at all, which is the case that needs a way back.
  const [setupFromLaunch, setSetupFromLaunch] = useState(false);

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

  // The season screen reads Sleeper's endpoints, so on a Yahoo league it can
  // only fail, forever. The header disables the button, and a remembered
  // "season" is read as the draft board until the league changes.
  const screen: Screen = view?.league.platform === "yahoo" ? "draft" : savedScreen;

  // The season screen's own data lifecycle: loads on first open, polls while
  // it is showing, and knows how to retry itself.
  const {
    season,
    error: seasonError,
    pollHealth: seasonPollHealth,
    retry: retrySeason,
  } = useSeasonSession(screen === "season", view?.league.league_id ?? null, showToast);
  const chime = useChime();
  // The confirm dialog and the one call behind it, including the guard that
  // stops a double tap sending the pick twice.
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
  // For the settings row and the picker's Yahoo lookup; the connect dialog
  // hands back every newer answer it is given, and the shell asks again as
  // the dialog closes and when the poller says Yahoo signed the user out.
  const yahoo = useYahooStatus(yahooOpen, pollHealth?.last_error ?? null);
  const updates = useUpdateRow();
  // Only the host has a server to ask about, and the answer is re-read as the
  // dialog closes so the row never contradicts what was just switched.
  const companionOn = useCompanionEnabled(follow === null, companionOpen);

  // Chime when the clock reaches you — the one moment worth interrupting for.
  usePickChime(view, chime);

  // ---------- actions ----------

  const { onDraft, onUndo } = useDraftEnd(view, askToDraft, undoLastPick, showToast);
  // Forget what the app decided about this draft's keepers and judge them
  // again. A league branded from one bad pick list stayed branded for ever.
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
    onClearKeepers: () => {
      setSettingsOpen(false);
      void clearKeepers();
    },
    // The username field lives on the setup screen and nowhere else; the
    // roster panel tells a Sleeper user to set it, so Settings has to be able
    // to get there after the first launch, with a way back.
    onSetUsername: () => {
      setSettingsOpen(false);
      setSetupFromLaunch(true);
      setShowSetup(true);
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
    onLeaveHost: leaveHost,
    onDiagnostics: () => {
      setSettingsOpen(false);
      setDiagnosticsOpen(true);
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
            followStatus={follow === null ? null : followStatus}
            onPairAgain={leaveHost}
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
            meta={headerMeta(view)}
            screen={screen}
            onScreen={setScreen}
            platform={view.league.platform}
            polling={polling}
            pollHealth={pollHealth}
            onRefreshPicks={() => void refreshPicks()}
            refreshingPicks={pullingPicks}
            onUndo={onUndo}
            chatOpen={chatOpen}
            onToggleChat={() => setChatOpen((c) => !c)}
            settingsOpen={settingsOpen}
            onToggleSettings={() => setSettingsOpen((s) => !s)}
            settingsRows={settingsRows}
            footerNote={footerNote(view)}
          />

          <ToastStrip toast={toast} onDismiss={dismissToast} />

          {/* Keyed per screen: one reused instance kept the season screen's
              crash on screen after switching to the draft, and back. */}
          {screen === "draft" ? (
            <ErrorBoundary key="draft">
              <Suspense fallback={<ScreenFallback />}>
                <DraftScreen view={view} busy={busy} onDraft={onDraft} />
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
