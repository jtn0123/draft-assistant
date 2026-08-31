import { Suspense, useCallback, useEffect, useRef, useState } from "react";
import { api } from "./api";
import { setAvatarMode, useAvatarMode } from "./avatars";
import { setChime, useChime } from "./prefs";
import { stableAvailable } from "./boardIdentity";
import { useSeasonSession } from "./session";
import type { DraftView, PollHealth, StoredLeague } from "./types";
import type { SeasonView } from "./season-types";
import { Header, type Screen, type SettingsRow } from "./components/Header";
import { Chat, DraftScreen, ScreenFallback, SeasonScreen } from "./components/lazyScreens";
import { LaunchScreen, Setup } from "./components/Panels";
import { ConfirmDialog, Toast } from "./components/Overlays";
import { ordinal, pickLabel, age, scoringFormat } from "./format";
import {
  applyTheme,
  resolveTheme,
  savePreference,
  storedPreference,
  watchSystemTheme,
  type ThemePreference,
} from "./theme";
import "./theme.css";
import "./App.css";
import "./components.css";
import "./board.css";
import "./season.css";
import "./season-tabs.css";
import "./trends.css";
import "./zoom.css";
import "./live.css";
import "./chat.css";

const MAX_RECONNECT_ATTEMPTS = 4;

type Confirm = { playerId: string; name: string } | null;

const SCREEN_KEY = "da.screen";

export default function App() {
  const [view, setView] = useState<DraftView | null>(null);
  // Season is the everyday screen; the draft is a few hours a year. The last
  // choice is remembered so a draft-night user lands back on the board.
  const [screen, setScreen] = useState<Screen>(() => {
    try {
      return localStorage.getItem(SCREEN_KEY) === "draft" ? "draft" : "season";
    } catch {
      return "season";
    }
  });
  const [polling, setPolling] = useState(false);
  const avatars = useAvatarMode();
  const [pollHealth, setPollHealth] = useState<PollHealth | null>(null);
  const [confirm, setConfirm] = useState<Confirm>(null);
  const [toast, setToast] = useState<string | null>(null);
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
  const [settingsOpen, setSettingsOpen] = useState(false);
  const [chatOpen, setChatOpen] = useState(false);
  const [preference, setPreference] = useState<ThemePreference>(storedPreference);
  const toastTimer = useRef<number | undefined>(undefined);
  const wasMyPick = useRef(false);

  // ---------- theme ----------

  useEffect(() => {
    applyTheme(resolveTheme(preference));
    savePreference(preference);
  }, [preference]);

  useEffect(() => {
    if (preference !== "system") return undefined;
    return watchSystemTheme(applyTheme);
  }, [preference]);

  const theme = resolveTheme(preference);

  // ---------- toasts ----------

  const showToast = useCallback((message: string) => {
    setToast(message);
    window.clearTimeout(toastTimer.current);
    toastTimer.current = window.setTimeout(() => setToast(null), 5000);
  }, []);

  useEffect(() => () => window.clearTimeout(toastTimer.current), []);

  // The season screen's own data lifecycle: loads on first open, polls while
  // it is showing, and knows how to retry itself.
  const {
    season,
    error: seasonError,
    retry: retrySeason,
  } = useSeasonSession(screen === "season", view !== null, showToast);
  const chime = useChime();

  // ---------- data ----------

  const applyView = useCallback((next: DraftView) => {
    // Hand the board back the array it has already sorted when the new view
    // says exactly the same thing about the players (see boardIdentity.ts).
    // Most updates move a clock or a status, not the several-hundred-row pool.
    setView((prev) => stableAvailable(prev, next));
    setPollHealth({
      last_success_at: next.data_health.poll_last_success_at ?? next.generated_at,
      consecutive_failures: next.data_health.poll_consecutive_failures ?? 0,
      last_error: next.data_health.poll_last_error ?? null,
    });
  }, []);

  const startLive = useCallback(async () => {
    try {
      await api.startPolling(3);
      setPolling(true);
    } catch (e) {
      showToast(String(e));
    }
  }, [showToast]);

  // Restore the last league on mount, and again whenever the user retries.
  // State is set from the promise callbacks, never synchronously in the body.
  useEffect(() => {
    let cancelled = false;
    api
      .getConfig()
      .then((config) => {
        if (cancelled) return null;
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
      void playChime();
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
      showToast(String(e));
    }
  };

  const doDraft = async (playerId: string) => {
    try {
      applyView(await api.recordManualPick(playerId));
    } catch (e) {
      showToast(String(e));
    } finally {
      setConfirm(null);
    }
  };

  const doUndo = async () => {
    try {
      applyView(await api.undoManualPick());
      showToast("Last recorded pick undone");
    } catch (e) {
      showToast(String(e));
    }
  };

  const doExport = async () => {
    setSettingsOpen(false);
    try {
      showToast(`State exported: ${await api.exportState()}`);
    } catch (e) {
      showToast(String(e));
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
      showToast(String(e));
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
      onSelect: () =>
        setPreference((p) =>
          // system -> light -> dark -> back to system
          p === "system" ? "light" : p === "light" ? "dark" : "system",
        ),
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
            onScreen={(next) => {
              setScreen(next);
              try {
                localStorage.setItem(SCREEN_KEY, next);
              } catch {
                // Private mode or a sandboxed webview: the choice just isn't remembered.
              }
            }}
            polling={polling}
            pollHealth={pollHealth}
            chime={chime}
            onToggleChime={() => setChime(!chime)}
            onUndo={() => void doUndo()}
            chatOpen={chatOpen}
            onToggleChat={() => setChatOpen((c) => !c)}
            settingsOpen={settingsOpen}
            onToggleSettings={() => setSettingsOpen((s) => !s)}
            settingsRows={settingsRows}
            footerNote={`${view.league.name} · league ${view.league.league_id} · read-only connection`}
          />

          {toast && <Toast message={toast} onDismiss={() => setToast(null)} />}

          {screen === "draft" ? (
            <Suspense fallback={<ScreenFallback />}>
              <DraftScreen view={view} busy={busy} onDraft={askToDraft} />
            </Suspense>
          ) : season !== null ? (
            <Suspense fallback={<ScreenFallback />}>
              <SeasonScreen view={season} />
            </Suspense>
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
          <Suspense fallback={null}>
            <Chat
              screen={screen}
              contextNote={
                screen === "season" && season !== null
                  ? `Sees week ${season.week} · your lineup and the league`
                  : `Sees this draft · pick ${pickLabel(d.current_pick, d.teams)}`
              }
              onClose={() => setChatOpen(false)}
            />
          </Suspense>
        )}
      </div>

      {confirm && (
        <ConfirmDialog
          pickLabel={`Pick ${pickLabel(d.current_pick, d.teams)} · slot ${d.on_clock_slot}`}
          playerName={confirm.name}
          onConfirm={() => void doDraft(confirm.playerId)}
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

/** A short two-tone chime via WebAudio — no asset to ship or fail to load. */
async function playChime(): Promise<void> {
  try {
    const Ctor = window.AudioContext ?? window.webkitAudioContext;
    if (Ctor === undefined) return;
    const ctx = new Ctor();
    const now = ctx.currentTime;
    for (const [i, freq] of [880, 1320].entries()) {
      const osc = ctx.createOscillator();
      const gain = ctx.createGain();
      osc.frequency.value = freq;
      osc.type = "sine";
      gain.gain.setValueAtTime(0.0001, now + i * 0.16);
      gain.gain.exponentialRampToValueAtTime(0.18, now + i * 0.16 + 0.02);
      gain.gain.exponentialRampToValueAtTime(0.0001, now + i * 0.16 + 0.15);
      osc.connect(gain).connect(ctx.destination);
      osc.start(now + i * 0.16);
      osc.stop(now + i * 0.16 + 0.16);
    }
    window.setTimeout(() => void ctx.close(), 600);
  } catch {
    // An audio failure must never interrupt the draft.
  }
}
