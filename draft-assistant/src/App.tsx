import { Suspense, useState } from "react";
import { api } from "./api";
import { useAppVersion } from "./appVersion";
import { setAvatarMode, useAvatarMode } from "./avatars";
import { MAX_RECONNECT_ATTEMPTS, useDraftSession } from "./draftSession";
import { useMarkDrafted } from "./markDrafted";
import { usePickChime } from "./pickChime";
import { setChime, setScreen, useChime, useScreen } from "./prefs";
import { importSecondOpinion } from "./secondOpinionImport";
import { clearFollow, readFollow, useCompanionEnabled, useFollowStatus } from "./companion";
import { useToast } from "./toast";
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
import { Diagnostics } from "./components/Diagnostics";
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

export default function App() {
  // Remembered between sessions, along with the rest of the preferences.
  const screen = useScreen();
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
  } = useMarkDrafted(applyView, showToast, follow?.host_name ?? null);
  // Asked once for the settings row and the picker's Yahoo lookup; the
  // connect dialog hands back every newer answer it is given.
  const yahoo = useYahooStatus();
  const appVersion = useAppVersion();
  // Only the host has a server to ask about, and the answer is re-read as the
  // dialog closes so the row never contradicts what was just switched.
  const companionOn = useCompanionEnabled(follow === null, companionOpen);

  // Chime when the clock reaches you — the one moment worth interrupting for.
  usePickChime(view, chime);

  // ---------- actions ----------

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

  if (showSetup) {
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
            Back to{" "}
            {restoring?.name === undefined || restoring.name === ""
              ? "the saved league"
              : restoring.name}
          </button>
        )}
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
            onDismiss={dismissToast}
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
          onDifferentLeague={() => {
            setSetupFromLaunch(true);
            setShowSetup(true);
          }}
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

  // Every screen is built against the host's data, so going home is a reload
  // rather than a state change — the same way joining was. The settings row
  // and the "Pair again" the header offers a revoked follower are one path.
  const leaveHost = () => {
    clearFollow();
    window.location.reload();
  };

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
              onDismiss={dismissToast}
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
        <Diagnostics appVersion={appVersion} onClose={() => setDiagnosticsOpen(false)} />
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

function myRecord(season: SeasonView): string {
  const mine = season.standings.find((s) => s.is_mine);
  if (mine === undefined) return `${season.standings.length} teams`;
  return `${mine.record} · ${ordinal(mine.seed)} of ${season.standings.length}`;
}
