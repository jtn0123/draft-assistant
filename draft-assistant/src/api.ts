import { invoke } from "@tauri-apps/api/core";
import { listen, type UnlistenFn } from "@tauri-apps/api/event";
import type {
  AppConfig,
  CompanionDevice,
  CompanionStatus,
  DraftView,
  PollHealth,
  SecondOpinionImport,
  SharedChatThread,
  StoredLeague,
  YahooConnectStart,
  YahooStatus,
} from "./types";
import type { SeasonView } from "./season-types";
import type { ChatReply, ChatRequest, ChatSettings } from "./chat-types";
import { ReplayFeed, replaySource } from "./replay";
import { readFollow } from "./companion";
import { remoteApi } from "./apiRemote";

// Kept in step with DRAFT_SCHEMA_VERSION in src-tauri/src/view_types.rs and
// SEASON_SCHEMA_VERSION in src-tauri/src/season.rs. Bump both sides together
// with the fixtures in public/ — src-tauri/tests/fixture_shape.rs fails if the
// fixtures and the structs disagree about a single field.
const DRAFT_VIEW_SCHEMA_VERSION = "1.4";
const SEASON_VIEW_SCHEMA_VERSION = "1.3";

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

/** Every call the UI can make into the backend. Exported so the test harness
 *  can key its mock off this type instead of a hand-kept list of names. */
export interface Api {
  addLeague(leagueId: string, force?: boolean): Promise<DraftView>;
  setMyUsername(username: string): Promise<string>;
  getConfig(): Promise<AppConfig>;
  /** Every league the saved Sleeper account plays in that season. */
  sleeperLeagues(season: string): Promise<StoredLeague[]>;
  /** How far the Yahoo connection has got: whether an app's credentials are
   *  saved, and whether a token was ever swapped for them. */
  yahooStatus(): Promise<YahooStatus>;
  /** Keep the client id and secret from developer.yahoo.com. The secret is
   *  write-only — nothing hands it back. */
  yahooSaveCredentials(clientId: string, clientSecret: string): Promise<YahooStatus>;
  /** Start the OAuth dance. The backend also tries to open the browser; the
   *  URL comes back so the dialog can show it when that fails. */
  yahooBeginConnect(): Promise<YahooConnectStart>;
  /** Swap the code Yahoo showed the user for a token. */
  yahooFinishConnect(code: string, state: string): Promise<YahooStatus>;
  /** Forget the token. The saved credentials stay. */
  yahooDisconnect(forgetCredentials?: boolean): Promise<YahooStatus>;
  /** Every league the connected Yahoo account plays in. */
  yahooLeagues(): Promise<StoredLeague[]>;
  /** Drop a league from the picker's list; the one on screen is refused.
   *  Resolves to the list as it stands afterwards. */
  removeLeague(leagueId: string): Promise<StoredLeague[]>;
  getState(): Promise<DraftView>;
  refreshPicks(): Promise<DraftView>;
  refreshData(): Promise<DraftView>;
  recordManualPick(playerId: string): Promise<DraftView>;
  undoManualPick(): Promise<DraftView>;
  /** Forget the keepers this app worked out for the draft on screen and judge
   *  them again from the picks as they stand. Returns the rebuilt view. */
  clearKeepers(): Promise<DraftView>;
  exportState(): Promise<string>;
  /** Open the native file picker, import the chosen projections CSV, and
   *  return the counts plus the rebuilt view. `null` if the user cancelled. */
  importSecondOpinion(): Promise<SecondOpinionImport | null>;
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
  /** Store the dollar cap a screen's chats run under; 0 turns it off. Returns
   *  the cap the backend kept. */
  setChatBudget(dollars: number): Promise<number>;
  chatSettings(): Promise<ChatSettings>;
  chatSuggestions(screen: string): Promise<string[]>;
  askClaude(args: ChatRequest): Promise<ChatReply>;

  // ---------- phone & second screen (COMPANION-API.md) ----------

  /** Whether the companion server is up, and on what URL and code. */
  companionStatus(): Promise<CompanionStatus>;
  /** Start listening on the LAN. Resolves to the status that follows. */
  companionEnable(): Promise<CompanionStatus>;
  companionDisable(): Promise<CompanionStatus>;
  /** New pairing code; every paired device is dropped. */
  companionRevoke(): Promise<CompanionStatus>;
  /** The name this machine signs its shared-chat messages with. Resolves to
   *  the name that was actually kept. */
  setDeviceName(name: string): Promise<string>;
  /** The shared thread for one screen of the league on screen. */
  sharedChatGet(screen: string): Promise<SharedChatThread>;
  /** Ask on the shared thread as this device. The answer arrives on the
   *  `shared-chat` event, not here. */
  sharedChatSend(screen: string, text: string): Promise<void>;
  onSharedChat(handler: (thread: SharedChatThread) => void): Promise<UnlistenFn>;
  onCompanionDevices(handler: (devices: CompanionDevice[]) => void): Promise<UnlistenFn>;
}

const tauriApi: Api = {
  addLeague: (leagueId, force = false) => invokeView("add_league", { leagueId, force }),
  setMyUsername: (username) => invoke<string>("set_my_username", { username }),
  getConfig: () => invoke<AppConfig>("get_config"),
  sleeperLeagues: (season) => invoke<StoredLeague[]>("sleeper_leagues", { season }),
  removeLeague: (leagueId) => invoke<StoredLeague[]>("remove_league", { leagueId }),
  yahooStatus: () => invoke<YahooStatus>("yahoo_status"),
  yahooSaveCredentials: (clientId, clientSecret) =>
    invoke<YahooStatus>("yahoo_save_credentials", { clientId, clientSecret }),
  yahooBeginConnect: () => invoke<YahooConnectStart>("yahoo_begin_connect"),
  yahooFinishConnect: (code, state) => invoke<YahooStatus>("yahoo_finish_connect", { code, state }),
  yahooDisconnect: (forgetCredentials = false) =>
    invoke<YahooStatus>("yahoo_disconnect", { forgetCredentials }),
  yahooLeagues: () => invoke<StoredLeague[]>("yahoo_leagues"),
  getState: () => invokeView("get_state"),
  refreshPicks: () => invokeView("refresh_picks"),
  refreshData: () => invokeView("refresh_data"),
  recordManualPick: (playerId) => invokeView("record_manual_pick", { playerId }),
  undoManualPick: () => invokeView("undo_manual_pick"),
  clearKeepers: () => invokeView("clear_keepers"),
  exportState: () => invoke<string>("export_state"),
  importSecondOpinion: async () => {
    const result = await invoke<SecondOpinionImport | null>("import_second_opinion");
    if (result === null) return null;
    return { ...result, view: validateDraftView(result.view) };
  },
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
  setChatBudget: (dollars) => invoke<number>("set_chat_budget", { dollars }),
  chatSettings: () => invoke<ChatSettings>("chat_settings"),
  chatSuggestions: (screen) => invoke<string[]>("chat_suggestions", { screen }),
  askClaude: (args) => invoke<ChatReply>("ask_claude", args),
  companionStatus: () => invoke<CompanionStatus>("companion_status"),
  companionEnable: () => invoke<CompanionStatus>("companion_enable"),
  companionDisable: () => invoke<CompanionStatus>("companion_disable"),
  companionRevoke: () => invoke<CompanionStatus>("companion_revoke"),
  setDeviceName: (name) => invoke<string>("set_device_name", { name }),
  sharedChatGet: (screen) => invoke<SharedChatThread>("shared_chat_get", { screen }),
  sharedChatSend: (screen, text) => invoke<void>("shared_chat_send", { screen, text }),
  onSharedChat: (handler) =>
    listen<SharedChatThread>("shared-chat", (event) => handler(event.payload)),
  onCompanionDevices: (handler) =>
    listen<CompanionDevice[]>("companion-devices", (event) => handler(event.payload)),
};

/**
 * Browser fallback for UI development: serves a captured real state dump
 * (public/dev-fixture.json) so the full interface renders outside Tauri.
 * Mutating calls only simulate what they can locally.
 *
 * `?replay=<url>` and `?replay-season=<url>` point either dump somewhere else
 * — at a file `scripts/replay-sleeper.mjs` keeps rewriting, typically — and
 * that turns the preview live: the source is re-read on a timer and every
 * newer dump reaches the screens through the very listeners the desktop
 * poller feeds. Without those parameters nothing changes: two fixtures, read
 * once, and live sync says it needs the desktop app.
 */
function browserApi(): Api {
  // A rejected promise, not a synchronous throw: callers of these methods
  // always `await` or `.catch()`, and the preview must fail the same way the
  // desktop app does.
  const readOnly = (advice: string): Promise<never> =>
    Promise.reject(new Error(`browser preview is read-only — ${advice}`));
  // Yahoo needs a keychain and a browser the app can open, so every step of
  // it says the same thing rather than half-working.
  const needsDesktop = (): Promise<never> =>
    Promise.reject(new Error("Yahoo needs the desktop app"));
  const search = window.location.search;
  const draft = new ReplayFeed<DraftView>({
    source: replaySource(search, "replay", "/dev-fixture.json"),
    missing: "dev fixture missing (browser preview only works with public/dev-fixture.json)",
    what: "draft state",
    validate: validateDraftView,
    generatedAt: (view) => view.generated_at,
  });
  const season = new ReplayFeed<SeasonView>({
    source: replaySource(search, "replay-season", "/dev-season-fixture.json"),
    missing: "season fixture missing (browser preview needs public/dev-season-fixture.json)",
    what: "season scores",
    validate: validateSeasonView,
    generatedAt: (view) => view.generated_at,
  });
  const fixture = () => draft.current();
  const seasonFixture = () => season.current();
  // Live sync in the preview means the replay timer; with no replay source
  // there is nothing to poll, and the preview says so as it always did.
  const startFeed = <V>(feed: ReplayFeed<V>, advice: string): Promise<void> => {
    if (!feed.live) return readOnly(advice);
    feed.start();
    return Promise.resolve();
  };
  return {
    addLeague: fixture,
    setMyUsername: (u) => Promise.resolve(u),
    getConfig: async () => {
      const v = await fixture();
      return {
        my_user_id: "browser-preview",
        active_league_id: v.league.league_id,
        leagues: [previewLeague(v)],
      };
    },
    sleeperLeagues: async () => [previewLeague(await fixture())],
    // Nothing is configured, so the dialog opens on its first step and says
    // what the rest of it needs.
    yahooStatus: () =>
      Promise.resolve({ configured: false, connected: false, redirect: "oob", account: null }),
    yahooSaveCredentials: () => needsDesktop(),
    yahooBeginConnect: () => needsDesktop(),
    yahooFinishConnect: () => needsDesktop(),
    yahooDisconnect: () => needsDesktop(),
    yahooLeagues: () => needsDesktop(),
    // The preview has one league and it is always on screen.
    removeLeague: () => Promise.reject(new Error("that league is on screen")),
    getState: fixture,
    refreshPicks: () => draft.refresh(),
    // Only the dump can be re-read here; the projections behind it are the
    // engine's, and the preview has no engine.
    refreshData: () => draft.refresh(),
    recordManualPick: () => readOnly("run the desktop app to draft"),
    undoManualPick: () => readOnly("run the desktop app to draft"),
    clearKeepers: () => readOnly("run the desktop app to draft"),
    exportState: () => Promise.resolve("browser preview — no export"),
    importSecondOpinion: () => readOnly("importing a CSV requires the desktop app"),
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
    startPolling: () => startFeed(draft, "live sync requires the desktop app"),
    stopPolling: () => {
      draft.stop();
      return Promise.resolve();
    },
    onDraftUpdated: (handler) => Promise.resolve(draft.onView(handler)),
    onPollHealth: (handler) => Promise.resolve(draft.onHealth(handler)),
    loadSeason: seasonFixture,
    getSeason: seasonFixture,
    refreshSeason: () => season.refresh(),
    startSeasonPolling: () => startFeed(season, "live scoring requires the desktop app"),
    stopSeasonPolling: () => {
      season.stop();
      return Promise.resolve();
    },
    onSeasonUpdated: (handler) => Promise.resolve(season.onView(handler)),
    onSeasonPollHealth: (handler) => Promise.resolve(season.onHealth(handler)),
    setApiKey: () => Promise.resolve(false),
    setChatProvider: () => Promise.resolve("api"),
    setChatBudget: (dollars) => Promise.resolve(dollars),
    chatSettings: () =>
      Promise.resolve({
        has_key: false,
        key_hint: null,
        cli_available: false,
        provider: "api",
        key_store: "file",
        budget_usd: 5,
        spend_usd: {},
        models: ["Opus 5", "Fable 5"],
        efforts: {
          "Opus 5": ["Off", "Low", "Medium", "High", "xhigh", "Max"],
          "Fable 5": ["Low", "Medium", "High", "xhigh", "Max"],
        },
        notes: {},
      }),
    chatSuggestions: () => Promise.resolve([]),
    askClaude: () => readOnly("Ask Claude requires the desktop app"),
    // The preview has no LAN server, so the panel is shown with plausible
    // fixtures: turning it on flips a variable in this closure and nothing
    // else, which is enough to lay the dialog out and read its copy.
    companionStatus: () => Promise.resolve(previewCompanion()),
    companionEnable: () => {
      previewEnabled = true;
      return Promise.resolve(previewCompanion());
    },
    companionDisable: () => {
      previewEnabled = false;
      return Promise.resolve(previewCompanion());
    },
    companionRevoke: () => {
      previewCode = String(100000 + Math.floor(Math.random() * 900000));
      return Promise.resolve(previewCompanion());
    },
    setDeviceName: (name) => {
      previewHostName = name.trim() === "" ? previewHostName : name.trim();
      return Promise.resolve(previewHostName);
    },
    sharedChatGet: async (screen) => ({
      league_id: (await fixture()).league.league_id,
      screen,
      busy: false,
      entries: [],
    }),
    sharedChatSend: () => readOnly("the shared chat needs the desktop app"),
    onSharedChat: () => Promise.resolve(() => undefined),
    onCompanionDevices: () => Promise.resolve(() => undefined),
  };
}

// The preview's companion server, such as it is: three variables the dialog
// can move, so its copy and layout are reviewable outside the shell.
let previewEnabled = false;
let previewCode = "418902";
let previewHostName = "This browser";

function previewCompanion(): CompanionStatus {
  return {
    enabled: previewEnabled,
    url: previewEnabled ? "http://192.168.1.24:7878/" : "",
    code: previewCode,
    port: 7878,
    host_name: previewHostName,
    devices: previewEnabled
      ? [
          {
            device_id: "preview-phone",
            name: "Rob's iPhone",
            kind: "phone",
            paired_at_ms: Date.now() - 600000,
            last_seen_ms: Date.now() - 4000,
            connected: true,
          },
        ]
      : [],
  };
}

/** The one league the preview has, as the picker's list wants it. */
function previewLeague(v: DraftView): StoredLeague {
  return {
    league_id: v.league.league_id,
    name: v.league.name,
    season: v.league.season,
    status: null,
    // A fixture captured before platforms existed says nothing; it is a
    // Sleeper dump either way.
    platform: v.league.platform ?? "sleeper",
  };
}

/** The host this window follows, read once as the module is created — before
 *  the Tauri-or-browser choice, because a follower talks to another app's
 *  server whichever shell it is running in. */
const follow = readFollow();

export const api: Api = follow !== null ? remoteApi(follow) : inTauri ? tauriApi : browserApi();
