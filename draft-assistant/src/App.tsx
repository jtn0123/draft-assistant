import { useCallback, useEffect, useRef, useState } from "react";
import { api } from "./api";
import type { DraftView, PollHealth } from "./types";
import { Board } from "./components/Board";
import { ClockBanner, RecCard, SidePanel, Setup } from "./components/Panels";
import "./App.css";
import "./components.css";

// ---------- app ----------

type Confirm = { playerId: string; name: string } | null;

export default function App() {
  const [view, setView] = useState<DraftView | null>(null);
  const [polling, setPolling] = useState(false);
  const [pollHealth, setPollHealth] = useState<PollHealth | null>(null);
  const [confirm, setConfirmState] = useState<Confirm>(null);
  const [toast, setToast] = useState<string | null>(null);
  const [busy, setBusy] = useState(true);
  const toastTimer = useRef<number | undefined>(undefined);
  // Highest view seq already rendered. The 3s poll and the awaited click
  // handlers both push views with no ordering guarantee, so without this a
  // poll that started earlier can land later and overwrite a fresher result.
  const appliedSeq = useRef(0);
  // Mirrors `confirm` so the update callback below can read the pending pick
  // without re-subscribing to the poller on every open/close.
  const confirmRef = useRef<Confirm>(null);

  const setConfirm = useCallback((next: Confirm) => {
    confirmRef.current = next;
    setConfirmState(next);
  }, []);

  const showToast = useCallback((message: string) => {
    setToast(message);
    window.clearTimeout(toastTimer.current);
    toastTimer.current = window.setTimeout(() => setToast(null), 4000);
  }, []);

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
      showToast(`${pending.name} was drafted by another team — pick cancelled`);
    }
    setPollHealth({
      last_success_at:
        next.data_health.poll_last_success_at ?? next.generated_at,
      consecutive_failures:
        next.data_health.poll_consecutive_failures ?? 0,
      last_error: next.data_health.poll_last_error ?? null,
    });
  }, [setConfirm, showToast]);

  const startLive = useCallback(async () => {
    try {
      await api.startPolling(3);
      setPolling(true);
    } catch (e) {
      showToast(String(e));
    }
  }, [showToast]);

  // Restore last league on launch; live sync starts automatically.
  useEffect(() => {
    (async () => {
      try {
        const config = await api.getConfig();
        if (config.active_league_id) {
          setBusy(true);
          const v = await api.addLeague(config.active_league_id);
          applyView(v);
          await startLive();
        }
      } catch (e) {
        showToast(String(e));
      } finally {
        setBusy(false);
      }
    })();
  }, [showToast, startLive, applyView]);

  // Live updates from the poller.
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
      const v = await api.recordManualPick(playerId);
      // Clear first so the stale-pick effect above cannot fire on our own pick.
      setConfirm(null);
      applyView(v);
    } catch (e) {
      showToast(String(e));
      setConfirm(null);
    }
  };

  const doUndo = async () => {
    try {
      applyView(await api.undoManualPick());
    } catch (e) {
      showToast(String(e));
    }
  };

  const doExport = async () => {
    try {
      const path = await api.exportState();
      showToast(`State exported: ${path}`);
    } catch (e) {
      showToast(String(e));
    }
  };

  const doRefreshData = async () => {
    setBusy(true);
    try {
      applyView(await api.refreshData());
      showToast("Projections refreshed and board rebuilt");
    } catch (e) {
      showToast(String(e));
    } finally {
      setBusy(false);
    }
  };

  if (view === null) {
    return busy ? (
      <div className="setup">
        <h1>Draft Assistant</h1>
        <p className="muted">Loading your league…</p>
      </div>
    ) : (
      <Setup
        onReady={(v) => {
          applyView(v);
          void startLive();
        }}
      />
    );
  }

  return (
    <div className="app">
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
          <button className="ghost" onClick={doUndo} title="Undo last manual pick">
            Undo
          </button>
          <button className="ghost" onClick={doExport} title="Write full draft state JSON for the AI">
            Export state
          </button>
          <button className="ghost" onClick={doRefreshData} disabled={busy} title="Re-fetch projections and rebuild the board">
            {busy ? "Refreshing…" : "Refresh data"}
          </button>
        </div>
      </header>

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
        />
      </main>

      {confirm && (
        <div className="modal-backdrop" onClick={() => setConfirm(null)}>
          <div className="modal" onClick={(e) => e.stopPropagation()}>
            <p>
              Mark <strong>{confirm.name}</strong> as drafted at pick{" "}
              {view.draft.current_pick} (slot {view.draft.on_clock_slot})?
            </p>
            <p className="muted small-text">
              Manual picks are a fallback — live sync from Sleeper overrides them.
            </p>
            <div className="modal-actions">
              <button onClick={() => doDraft(confirm.playerId)}>Confirm</button>
              <button className="ghost" onClick={() => setConfirm(null)}>
                Cancel
              </button>
            </div>
          </div>
        </div>
      )}

      {toast && <div className="toast">{toast}</div>}
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
