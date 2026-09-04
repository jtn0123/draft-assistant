import { Suspense, useCallback, useEffect, useRef, useState } from "react";
import { api } from "./api";
import { setAvatarMode, useAvatarMode } from "./avatars";
import { playChime } from "./chime";
import { MAX_RECONNECT_ATTEMPTS, useDraftSession } from "./draftSession";
import { setChime, setScreen, useChime, useScreen } from "./prefs";
import { importNote, importSecondOpinion } from "./secondOpinionImport";
import { useSeasonSession } from "./session";
import type { SeasonView } from "./season-types";
import { Header, type SettingsRow } from "./components/Header";
import { Chat, DraftScreen, ScreenFallback, SeasonScreen } from "./components/lazyScreens";
import { LaunchScreen, Setup } from "./components/Panels";
import { LeaguePicker } from "./components/LeaguePicker";
import { YahooConnect } from "./components/YahooConnect";
import { ConfirmDialog, Toast } from "./components/Overlays";
import { ErrorBoundary } from "./components/ErrorBoundary";
import { ordinal, pickLabel, age, problem, scoringFormat } from "./format";
import { cycleThemePreference, useAppliedTheme } from "./theme";
import { useYahooStatus, yahooNote } from "./yahoo";
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

/** A line under the header. `retry` marks it as something gone wrong that the
 * user can have another go at — and that should wait for them. */
type ToastMessage = { text: string; retry?: () => void };

export default function App() {
  // Remembered between sessions, along with the rest of the preferences.
  const screen = useScreen();
  const avatars = useAvatarMode();
  const [confirm, setConfirm] = useState<Confirm>(null);
  const [toast, setToast] = useState<ToastMessage | null>(null);
  // Stable across renders so the memoised board rows are not invalidated by a
  // fresh closure on every 3-second poll.
  const askToDraft = useCallback(
    (playerId: string, name: string) => setConfirm({ playerId, name }),
    [],
  );
  const [leaguePicker, setLeaguePicker] = useState(false);
  const [yahooOpen, setYahooOpen] = useState(false);
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

  if (showSetup) {
    return (
      <div className="app">
        <Setup
          onReady={(v) => {
            applyView(v);
            setShowSetup(false);
            void refreshLeagues();
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
      label: "Yahoo",
      note: yahooNote(yahoo.status),
      value: yahoo.connected ? "Connected" : "Connect",
      on: yahoo.connected,
      onSelect: () => {
        setSettingsOpen(false);
        setYahooOpen(true);
      },
    },
    {
      label: "Refresh data",
      note: "Re-fetch projections and rebuild the board",
      value: busy ? "…" : "Sync",
      on: false,
      onSelect: () => {
        setSettingsOpen(false);
        void refreshData(() => {
          // The season view is built on the old projections until it reloads.
          if (screen === "season") retrySeason();
        });
      },
    },
    {
      label: "Export state",
      note: "Full JSON dump of everything on screen",
      value: "JSON",
      on: false,
      onSelect: () => {
        setSettingsOpen(false);
        void exportState();
      },
    },
    {
      label: "Import projections CSV…",
      note: importNote(
        view.data_health.second_opinion_loaded_at,
        view.available.find((p) => p.second_opinion !== null)?.second_opinion?.source ?? null,
      ),
      value: "Choose",
      on: view.data_health.second_opinion_loaded_at !== null,
      onSelect: () => {
        setSettingsOpen(false);
        void importSecondOpinion(applyView, showToast);
      },
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
            onSwitchLeague={() => setLeaguePicker(true)}
            subtitle={subtitle}
            meta={`${d.teams}-team ${scoringFormat(view.league.scoring_settings.rec)} · ${d.rounds} rounds${d.manual_picks_active ? " · manual picks active" : ""}`}
            screen={screen}
            onScreen={setScreen}
            polling={polling}
            pollHealth={pollHealth}
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
