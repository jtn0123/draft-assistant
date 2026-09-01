import { invoke } from "@tauri-apps/api/core";
import { listen, type UnlistenFn } from "@tauri-apps/api/event";
import type { AppConfig, DraftView, PollHealth, StoredLeague } from "./types";
import type { SeasonView } from "./season-types";
import type { ChatReply, ChatRequest, ChatSettings } from "./chat-types";

const DRAFT_VIEW_SCHEMA_VERSION = "1.1";
const SEASON_VIEW_SCHEMA_VERSION = "1.0";

export function validateDraftView(value: DraftView): DraftView {
  if (value.schema_version !== DRAFT_VIEW_SCHEMA_VERSION) {
    throw new Error(
      `Incompatible draft data: expected schema ${DRAFT_VIEW_SCHEMA_VERSION}, received ${value.schema_version || "missing"}. Update and restart the app.`,
    );
  }
  return value;
}

export function validateSeasonView(value: SeasonView): SeasonView {
  if (value.schema_version !== SEASON_VIEW_SCHEMA_VERSION) {
    throw new Error(
      `Incompatible season data: expected schema ${SEASON_VIEW_SCHEMA_VERSION}, received ${value.schema_version || "missing"}. Update and restart the app.`,
    );
  }
  return value;
}

async function invokeView(command: string, args?: Record<string, unknown>): Promise<DraftView> {
  return validateDraftView(await invoke<DraftView>(command, args));
}

async function invokeSeason(command: string, args?: Record<string, unknown>): Promise<SeasonView> {
  return validateSeasonView(await invoke<SeasonView>(command, args));
}

/** True when running inside the Tauri shell (vs a plain browser tab). */
const inTauri = "__TAURI_INTERNALS__" in window;

interface Api {
  addLeague(leagueId: string, force?: boolean): Promise<DraftView>;
  setMyUsername(username: string): Promise<string>;
  getConfig(): Promise<AppConfig>;
  /** Every league the saved Sleeper account plays in that season. */
  sleeperLeagues(season: string): Promise<StoredLeague[]>;
  getState(): Promise<DraftView>;
  refreshPicks(): Promise<DraftView>;
  refreshData(): Promise<DraftView>;
  recordManualPick(playerId: string): Promise<DraftView>;
  undoManualPick(): Promise<DraftView>;
  exportState(): Promise<string>;
  /** Player photo as a data URL, cached on disk by the backend; null if none. */
  headshot(playerId: string): Promise<string | null>;
  /** Manager's team picture as a data URL, cached on disk; null if none. */
  avatar(reference: string, full: boolean): Promise<string | null>;
  startPolling(intervalSecs?: number): Promise<void>;
  stopPolling(): Promise<void>;
  onDraftUpdated(handler: (view: DraftView) => void): Promise<UnlistenFn>;
  onPollHealth(handler: (health: PollHealth) => void): Promise<UnlistenFn>;
  loadSeason(force?: boolean): Promise<SeasonView>;
  getSeason(): Promise<SeasonView>;
  refreshSeason(): Promise<SeasonView>;
  startSeasonPolling(intervalSecs?: number): Promise<void>;
  stopSeasonPolling(): Promise<void>;
  onSeasonUpdated(handler: (view: SeasonView) => void): Promise<UnlistenFn>;
  /** How the season poller's last attempt went — the live-scoring twin of
   *  `onPollHealth`, which reports the draft poller. */
  onSeasonPollHealth(handler: (health: PollHealth) => void): Promise<UnlistenFn>;
  setApiKey(key: string): Promise<boolean>;
  setChatProvider(provider: "api" | "claude_code"): Promise<"api" | "claude_code">;
  chatSettings(): Promise<ChatSettings>;
  chatSuggestions(screen: string): Promise<string[]>;
  askClaude(args: ChatRequest): Promise<ChatReply>;
}

const tauriApi: Api = {
  addLeague: (leagueId, force = false) => invokeView("add_league", { leagueId, force }),
  setMyUsername: (username) => invoke<string>("set_my_username", { username }),
  getConfig: () => invoke<AppConfig>("get_config"),
  sleeperLeagues: (season) => invoke<StoredLeague[]>("sleeper_leagues", { season }),
  getState: () => invokeView("get_state"),
  refreshPicks: () => invokeView("refresh_picks"),
  refreshData: () => invokeView("refresh_data"),
  recordManualPick: (playerId) => invokeView("record_manual_pick", { playerId }),
  undoManualPick: () => invokeView("undo_manual_pick"),
  exportState: () => invoke<string>("export_state"),
  headshot: (playerId) => invoke<string | null>("headshot", { playerId }),
  avatar: (reference, full) => invoke<string | null>("avatar", { reference, full }),
  startPolling: (intervalSecs = 3) => invoke<void>("start_polling", { intervalSecs }),
  stopPolling: () => invoke<void>("stop_polling"),
  onDraftUpdated: (handler) =>
    listen<DraftView>("draft-updated", (event) => handler(validateDraftView(event.payload))),
  onPollHealth: (handler) => listen<PollHealth>("poll-health", (event) => handler(event.payload)),
  loadSeason: (force = false) => invokeSeason("load_season", { force }),
  getSeason: () => invokeSeason("get_season"),
  refreshSeason: () => invokeSeason("refresh_season"),
  startSeasonPolling: (intervalSecs = 30) => invoke<void>("start_season_polling", { intervalSecs }),
  stopSeasonPolling: () => invoke<void>("stop_season_polling"),
  onSeasonUpdated: (handler) =>
    listen<SeasonView>("season-updated", (event) => handler(validateSeasonView(event.payload))),
  onSeasonPollHealth: (handler) =>
    listen<PollHealth>("season-poll-health", (event) => handler(event.payload)),
  setApiKey: (key) => invoke<boolean>("set_api_key", { key }),
  setChatProvider: (provider) => invoke<"api" | "claude_code">("set_chat_provider", { provider }),
  chatSettings: () => invoke<ChatSettings>("chat_settings"),
  chatSuggestions: (screen) => invoke<string[]>("chat_suggestions", { screen }),
  askClaude: (args) => invoke<ChatReply>("ask_claude", args),
};

/**
 * Browser fallback for UI development: serves a captured real state dump
 * (public/dev-fixture.json) so the full interface renders outside Tauri.
 * Mutating calls only simulate what they can locally.
 */
function browserApi(): Api {
  // A rejected promise, not a synchronous throw: callers of these methods
  // always `await` or `.catch()`, and the preview must fail the same way the
  // desktop app does.
  const readOnly = (advice: string): Promise<never> =>
    Promise.reject(new Error(`browser preview is read-only — ${advice}`));
  let cached: DraftView | null = null;
  let cachedSeason: SeasonView | null = null;
  const seasonFixture = async (): Promise<SeasonView> => {
    if (cachedSeason === null) {
      const resp = await fetch("/dev-season-fixture.json");
      if (!resp.ok) {
        throw new Error(
          "season fixture missing (browser preview needs public/dev-season-fixture.json)",
        );
      }
      cachedSeason = validateSeasonView((await resp.json()) as SeasonView);
    }
    return cachedSeason;
  };
  const fixture = async (): Promise<DraftView> => {
    if (cached === null) {
      const resp = await fetch("/dev-fixture.json");
      if (!resp.ok)
        throw new Error(
          "dev fixture missing (browser preview only works with public/dev-fixture.json)",
        );
      cached = validateDraftView((await resp.json()) as DraftView);
    }
    return cached;
  };
  return {
    addLeague: fixture,
    setMyUsername: (u) => Promise.resolve(u),
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
    sleeperLeagues: async () => {
      const v = await fixture();
      return [{ league_id: v.league.league_id, name: v.league.name, season: v.league.season }];
    },
    getState: fixture,
    refreshPicks: fixture,
    refreshData: fixture,
    recordManualPick: () => readOnly("run the desktop app to draft"),
    undoManualPick: () => readOnly("run the desktop app to draft"),
    exportState: () => Promise.resolve("browser preview — no export"),
    headshot: (playerId) =>
      Promise.resolve(
        /^\d+$/.test(playerId)
          ? `https://sleepercdn.com/content/nfl/players/thumb/${playerId}.jpg`
          : null,
      ),
    avatar: (reference, full) =>
      Promise.resolve(
        reference.startsWith("https://sleepercdn.com/")
          ? reference
          : /^[0-9a-f]+$/.test(reference)
            ? `https://sleepercdn.com/avatars/${full ? "" : "thumbs/"}${reference}`
            : null,
      ),
    startPolling: () => readOnly("live sync requires the desktop app"),
    stopPolling: () => Promise.resolve(),
    onDraftUpdated: () => Promise.resolve(() => undefined),
    onPollHealth: () => Promise.resolve(() => undefined),
    loadSeason: seasonFixture,
    getSeason: seasonFixture,
    refreshSeason: seasonFixture,
    startSeasonPolling: () => readOnly("live scoring requires the desktop app"),
    stopSeasonPolling: () => Promise.resolve(),
    onSeasonUpdated: () => Promise.resolve(() => undefined),
    onSeasonPollHealth: () => Promise.resolve(() => undefined),
    setApiKey: () => Promise.resolve(false),
    setChatProvider: () => Promise.resolve("api"),
    chatSettings: () =>
      Promise.resolve({
        has_key: false,
        key_hint: null,
        cli_available: false,
        provider: "api",
        models: ["Opus 5", "Fable 5"],
        efforts: {
          "Opus 5": ["Off", "Low", "Medium", "High", "xhigh", "Max"],
          "Fable 5": ["Low", "Medium", "High", "xhigh", "Max"],
        },
        notes: {},
      }),
    chatSuggestions: () => Promise.resolve([]),
    askClaude: () => readOnly("Ask Claude requires the desktop app"),
  };
}

export const api: Api = inTauri ? tauriApi : browserApi();
