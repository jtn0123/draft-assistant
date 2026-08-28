import { useCallback, useEffect, useRef, useState } from "react";
import { api } from "./api";
import type { DraftView, PollHealth } from "./types";
import { errorMessage } from "./format";
import { Board } from "./components/Board";
import { Chat } from "./components/Chat";
import { ConfirmDialog } from "./components/ConfirmDialog";
import { ClockBanner, RecCard, SidePanel, Setup } from "./components/Panels";
import "./App.css";
import "./components.css";
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
  const [chatOpen, setChatOpen] = useState(false);
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

  // Restore last league on launch; live sync starts automatically (except in
  // the browser preview, where there is nothing to sync).
  useEffect(() => {
    if (booted.current) return;
    booted.current = true;
    (async () => {
      try {
        const config = await api.getConfig();
        if (config.active_league_id) {
          setBusy(true);
          const v = await api.addLeague(config.active_league_id);
          applyView(v);
          if (!api.preview || api.replay) await startLive();
        }
      } catch (e) {
        fail(errorMessage(e));
      } finally {
        setBusy(false);
      }
    })();
  }, [fail, startLive, applyView]);

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
    return busy ? (
      <div className="setup">
        <h1>Draft Assistant</h1>
        <p className="muted">Loading your league…</p>
      </div>
    ) : (
      <>
        {alertBar}
        <Setup
          onReady={(v) => {
            applyView(v);
            if (!api.preview || api.replay) void startLive();
          }}
        />
      </>
    );
  }

  // The chat is a column beside the page, not an overlay: the numbers being
  // asked about stay visible next to the answer.
  return (
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
            {view.league.season} · {view.draft.teams} teams · {view.draft.rounds} rounds
            {view.draft.manual_picks_active && " · manual picks active"}
          </span>
        </div>
        <div className="actions">
          <button
            className={`live ${syncClass(polling, pollHealth)}`}
            onClick={togglePolling}
            title={pollHealth?.last_error ?? undefined}
          >
            {syncLabel(polling, pollHealth)}
          </button>
          {polling && pollHealth?.last_success_at && (
            <span className="sync-age">
              Last sync {formatAge(pollHealth.last_success_at)}
            </span>
          )}
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
            className={`ghost ${chatOpen ? "on" : ""}`}
            onClick={() => setChatOpen((v) => !v)}
            title="Ask Claude about the current draft"
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

      <ClockBanner view={view} />

      {view.data_health.warnings.length > 0 && (
        <div className="warnings">{view.data_health.warnings.join(" · ")}</div>
      )}

      <div className="recs">
        {view.recommendations
          .filter(
            (r, i, all) =>
              i === all.findIndex((x) => x.player_id === r.player_id),
          )
          .map((r) => (
            <RecCard key={r.mode} rec={r} onDraft={(id, name) => setConfirm({ playerId: id, name })} />
          ))}
      </div>

      <main>
        <SidePanel view={view} />
        <Board
          players={view.available}
          positions={view.league.draftable_positions}
          onDraft={(id, name) => setConfirm({ playerId: id, name })}
          draftOver={view.draft.status === "complete"}
        />
      </main>

      {confirm && (
        <ConfirmDialog
          name={confirm.name}
          pick={view.draft.current_pick}
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
    <Chat open={chatOpen} onClose={() => setChatOpen(false)} />
    </div>
  );
}

function syncClass(polling: boolean, health: PollHealth | null): string {
  if (!polling) return "";
  if ((health?.consecutive_failures ?? 0) >= 2) return "stale";
  if ((health?.consecutive_failures ?? 0) === 1) return "retrying";
  return "on";
}

function syncLabel(polling: boolean, health: PollHealth | null): string {
  if (!polling) return "○ Live sync off";
  const failures = health?.consecutive_failures ?? 0;
  if (failures >= 2) return `● Sync stale · ${failures} failures`;
  if (failures === 1) return "● Sync retrying";
  return "● Live sync on";
}

function formatAge(timestamp: number): string {
  const seconds = Math.max(0, Math.floor(Date.now() / 1000 - timestamp));
  if (seconds < 60) return `${seconds}s ago`;
  return `${Math.floor(seconds / 60)}m ago`;
}
