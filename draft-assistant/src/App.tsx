import { useCallback, useEffect, useRef, useState } from "react";
import { api } from "./api";
import type { DraftView } from "./types";
import { Board } from "./components/Board";
import { ClockBanner, RecCard, SidePanel, Setup } from "./components/Panels";
import "./App.css";
import "./components.css";

// ---------- app ----------

type Confirm = { playerId: string; name: string } | null;

export default function App() {
  const [view, setView] = useState<DraftView | null>(null);
  const [polling, setPolling] = useState(false);
  const [confirm, setConfirm] = useState<Confirm>(null);
  const [toast, setToast] = useState<string | null>(null);
  const [busy, setBusy] = useState(false);
  const toastTimer = useRef<number | undefined>(undefined);

  const showToast = useCallback((message: string) => {
    setToast(message);
    window.clearTimeout(toastTimer.current);
    toastTimer.current = window.setTimeout(() => setToast(null), 4000);
  }, []);

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
          setView(v);
          await startLive();
        }
      } catch (e) {
        showToast(String(e));
      } finally {
        setBusy(false);
      }
    })();
  }, [showToast, startLive]);

  // Live updates from the poller.
  useEffect(() => {
    const un = api.onDraftUpdated((v) => setView(v));
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
      setView(v);
      setConfirm(null);
    } catch (e) {
      showToast(String(e));
      setConfirm(null);
    }
  };

  const doUndo = async () => {
    try {
      setView(await api.undoManualPick());
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
      setView(await api.refreshData());
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
          setView(v);
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
          <button className={polling ? "live on" : "live"} onClick={togglePolling}>
            {polling ? "● Live sync on" : "○ Live sync off"}
          </button>
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
        <Board players={view.available} onDraft={(id, name) => setConfirm({ playerId: id, name })} />
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
