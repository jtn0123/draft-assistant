import { invoke } from "@tauri-apps/api/core";
import { listen, type UnlistenFn } from "@tauri-apps/api/event";
import type { AppConfig, DraftView, PollHealth } from "./types";

const DRAFT_VIEW_SCHEMA_VERSION = "1.2";

export function validateDraftView(value: DraftView): DraftView {
  if (value.schema_version !== DRAFT_VIEW_SCHEMA_VERSION) {
    throw new Error(
      `Incompatible draft data: expected schema ${DRAFT_VIEW_SCHEMA_VERSION}, received ${value.schema_version || "missing"}. Update and restart the app.`,
    );
  }
  return value;
}

async function invokeView(command: string, args?: Record<string, unknown>): Promise<DraftView> {
  return validateDraftView(await invoke<DraftView>(command, args));
}

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
  chat(question: string): Promise<string>;
  startPolling(intervalSecs?: number): Promise<void>;
  stopPolling(): Promise<void>;
  /**
   * Live views from the poller. A payload that fails the schema check is
   * reported through `onError` — thrown inside an event callback it would be
   * swallowed, and live updates would silently stop.
   */
  onDraftUpdated(
    handler: (view: DraftView) => void,
    onError?: (error: unknown) => void,
  ): Promise<UnlistenFn>;
  onPollHealth(handler: (health: PollHealth) => void): Promise<UnlistenFn>;
}

const tauriApi: Api = {
  addLeague: (leagueId, force = false) =>
    invokeView("add_league", { leagueId, force }),
  setMyUsername: (username) => invoke<string>("set_my_username", { username }),
  getConfig: () => invoke<AppConfig>("get_config"),
  getState: () => invokeView("get_state"),
  refreshPicks: () => invokeView("refresh_picks"),
  refreshData: () => invokeView("refresh_data"),
  recordManualPick: (playerId) =>
    invokeView("record_manual_pick", { playerId }),
  undoManualPick: () => invokeView("undo_manual_pick"),
  exportState: () => invoke<string>("export_state"),
  chat: (question) => invoke<string>("chat", { question }),
  startPolling: (intervalSecs = 3) =>
    invoke<void>("start_polling", { intervalSecs }),
  stopPolling: () => invoke<void>("stop_polling"),
  onDraftUpdated: (handler, onError) =>
    listen<DraftView>("draft-updated", (event) => {
      try {
        handler(validateDraftView(event.payload));
      } catch (error) {
        onError?.(error);
      }
    }),
  onPollHealth: (handler) =>
    listen<PollHealth>("poll-health", (event) => handler(event.payload)),
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
      cached = validateDraftView((await resp.json()) as DraftView);
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
    chat: async () => {
      throw new Error("browser preview cannot reach the Claude CLI — run the desktop app");
    },
    startPolling: async () => {
      throw new Error("browser preview is read-only — live sync requires the desktop app");
    },
    stopPolling: async () => undefined,
    onDraftUpdated: async () => () => undefined,
    onPollHealth: async () => () => undefined,
  };
}

export const api: Api = inTauri ? tauriApi : browserApi();
