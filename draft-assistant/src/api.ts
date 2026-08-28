import { invoke } from "@tauri-apps/api/core";
import { listen, type UnlistenFn } from "@tauri-apps/api/event";
import type { AppConfig, DraftView } from "./types";

/** True when running inside the Tauri shell (vs a plain browser tab). */
const inTauri = "__TAURI_INTERNALS__" in window;

interface Api {
  addLeague(leagueId: string, force?: boolean): Promise<DraftView>;
  setMyUsername(username: string): Promise<string>;
  getConfig(): Promise<AppConfig>;
  getState(): Promise<DraftView>;
  refreshPicks(): Promise<DraftView>;
  refreshData(): Promise<DraftView>;
  recordManualPick(playerId: string): Promise<DraftView>;
  undoManualPick(): Promise<DraftView>;
  exportState(): Promise<string>;
  startPolling(intervalSecs?: number): Promise<void>;
  stopPolling(): Promise<void>;
  onDraftUpdated(handler: (view: DraftView) => void): Promise<UnlistenFn>;
}

const tauriApi: Api = {
  addLeague: (leagueId, force = false) =>
    invoke<DraftView>("add_league", { leagueId, force }),
  setMyUsername: (username) => invoke<string>("set_my_username", { username }),
  getConfig: () => invoke<AppConfig>("get_config"),
  getState: () => invoke<DraftView>("get_state"),
  refreshPicks: () => invoke<DraftView>("refresh_picks"),
  refreshData: () => invoke<DraftView>("refresh_data"),
  recordManualPick: (playerId) =>
    invoke<DraftView>("record_manual_pick", { playerId }),
  undoManualPick: () => invoke<DraftView>("undo_manual_pick"),
  exportState: () => invoke<string>("export_state"),
  startPolling: (intervalSecs = 3) =>
    invoke<void>("start_polling", { intervalSecs }),
  stopPolling: () => invoke<void>("stop_polling"),
  onDraftUpdated: (handler) =>
    listen<DraftView>("draft-updated", (event) => handler(event.payload)),
};

/**
 * Browser fallback for UI development: serves a captured real state dump
 * (public/dev-fixture.json) so the full interface renders outside Tauri.
 * Mutating calls only simulate what they can locally.
 */
function browserApi(): Api {
  let cached: DraftView | null = null;
  const fixture = async (): Promise<DraftView> => {
    if (cached === null) {
      const resp = await fetch("/dev-fixture.json");
      if (!resp.ok) throw new Error("dev fixture missing (browser preview only works with public/dev-fixture.json)");
      cached = (await resp.json()) as DraftView;
    }
    return cached;
  };
  return {
    addLeague: fixture,
    setMyUsername: async (u) => u,
    getConfig: async () => {
      const v = await fixture();
      return {
        my_user_id: "browser-preview",
        active_league_id: v.league.league_id,
        leagues: [
          {
            league_id: v.league.league_id,
            name: v.league.name,
            season: v.league.season,
          },
        ],
      };
    },
    getState: fixture,
    refreshPicks: fixture,
    refreshData: fixture,
    recordManualPick: async () => {
      throw new Error("browser preview is read-only — run the desktop app to draft");
    },
    undoManualPick: async () => {
      throw new Error("browser preview is read-only — run the desktop app to draft");
    },
    exportState: async () => "browser preview — no export",
    startPolling: async () => undefined,
    stopPolling: async () => undefined,
    onDraftUpdated: async () => () => undefined,
  };
}

export const api: Api = inTauri ? tauriApi : browserApi();
