// The mocked backend the App tests drive, in one place.
//
// Every App test needs the same twenty-odd `api` methods stubbed and the same
// four event subscriptions capturing their handlers, and `vi.mock` factories
// cannot see anything the file imports normally. So the mock lives here as a
// module singleton and the factory reaches it with a dynamic import:
//
//   vi.mock("./api", async () => ({ api: (await import("./test/appHarness")).harness().api }));

import { vi, type Mock } from "vitest";
import draftFixtureJson from "../../public/dev-fixture.json";
import type { Api } from "../api";
import type {
  AppConfig,
  CompanionDevice,
  CompanionStatus,
  Diagnostics,
  DraftView,
  PollHealth,
  SharedChatThread,
} from "../types";
import type { SeasonView } from "../season-types";

/**
 * One stub per backend call, keyed off the real `Api` type.
 *
 * `Record<keyof Api, true>` is what makes this honest: adding a method to
 * `Api` without adding it here is a missing-property error, and leaving a
 * renamed one behind is an excess-property error. The list used to be a bare
 * array of strings, and had quietly fallen four methods behind.
 */
const METHODS: Record<keyof Api, true> = {
  addLeague: true,
  setMyUsername: true,
  getConfig: true,
  sleeperLeagues: true,
  removeLeague: true,
  yahooStatus: true,
  yahooSaveCredentials: true,
  yahooBeginConnect: true,
  yahooFinishConnect: true,
  yahooDisconnect: true,
  yahooLeagues: true,
  getState: true,
  refreshPicks: true,
  refreshData: true,
  recordManualPick: true,
  undoManualPick: true,
  clearKeepers: true,
  exportState: true,
  importSecondOpinion: true,
  headshot: true,
  avatar: true,
  startPolling: true,
  stopPolling: true,
  onDraftUpdated: true,
  onPollHealth: true,
  loadSeason: true,
  getSeason: true,
  refreshSeason: true,
  startSeasonPolling: true,
  stopSeasonPolling: true,
  onSeasonUpdated: true,
  onSeasonPollHealth: true,
  setApiKey: true,
  setChatProvider: true,
  setChatBudget: true,
  chatSettings: true,
  chatSuggestions: true,
  askClaude: true,
  companionStatus: true,
  companionEnable: true,
  companionDisable: true,
  companionRevoke: true,
  setDeviceName: true,
  sharedChatGet: true,
  sharedChatSend: true,
  sharedChatReset: true,
  onSharedChat: true,
  onCompanionDevices: true,
  diagnostics: true,
  openLogFolder: true,
  logFrontendError: true,
};

const METHOD_NAMES = Object.keys(METHODS) as (keyof Api)[];

export type ApiMock = Record<keyof Api, Mock>;

/** The handlers the app registered, so a test can push a backend event. */
export interface PushHandlers {
  draft: ((view: DraftView) => void) | null;
  health: ((health: PollHealth) => void) | null;
  season: ((view: SeasonView) => void) | null;
  seasonHealth: ((health: PollHealth) => void) | null;
  /** The shared thread the companion server pushes. */
  sharedChat: ((thread: SharedChatThread) => void) | null;
  /** The paired-device list, pushed whenever one connects or drops. */
  devices: ((devices: CompanionDevice[]) => void) | null;
}

export interface Harness {
  api: ApiMock;
  push: PushHandlers;
  /** Clear every call and reinstall the "nothing is wrong" defaults. */
  reset: () => void;
}

function build(): Harness {
  const api = Object.fromEntries(METHOD_NAMES.map((name) => [name, vi.fn()])) as ApiMock;
  const push: PushHandlers = {
    draft: null,
    health: null,
    season: null,
    seasonHealth: null,
    sharedChat: null,
    devices: null,
  };

  const reset = () => {
    for (const name of METHOD_NAMES) api[name].mockReset();
    push.draft = null;
    push.health = null;
    push.season = null;
    push.seasonHealth = null;
    push.sharedChat = null;
    push.devices = null;

    api.startPolling.mockResolvedValue(undefined);
    api.stopPolling.mockResolvedValue(undefined);
    api.startSeasonPolling.mockResolvedValue(undefined);
    api.stopSeasonPolling.mockResolvedValue(undefined);
    api.exportState.mockResolvedValue("/tmp/draft-state.json");
    api.headshot.mockResolvedValue(null);
    api.avatar.mockResolvedValue(null);
    api.chatSuggestions.mockResolvedValue([]);
    api.companionStatus.mockResolvedValue(companionStatus());
    api.companionEnable.mockResolvedValue(companionStatus({ enabled: true }));
    api.companionDisable.mockResolvedValue(companionStatus());
    api.companionRevoke.mockResolvedValue(companionStatus({ enabled: true, code: "654321" }));
    api.setDeviceName.mockImplementation((name: string) => Promise.resolve(name));
    api.sharedChatGet.mockImplementation((screen: string) =>
      Promise.resolve({ league_id: "1", screen, busy: false, entries: [] }),
    );
    api.sharedChatSend.mockResolvedValue(undefined);
    api.logFrontendError.mockResolvedValue(undefined);
    api.openLogFolder.mockResolvedValue("/tmp/logs");
    api.diagnostics.mockResolvedValue(diagnostics());
    api.sleeperLeagues.mockResolvedValue([]);
    api.yahooLeagues.mockResolvedValue([]);
    api.yahooStatus.mockResolvedValue({
      configured: false,
      connected: false,
      redirect: "oob",
      account: null,
    });
    api.chatSettings.mockResolvedValue({
      cli_available: false,
      provider: "api",
      has_key: false,
      key_hint: null,
      models: ["Opus 5", "Fable 5"],
      efforts: { "Opus 5": ["Off", "High"], "Fable 5": ["Low", "High"] },
      notes: {},
    });

    // Typed handlers rather than inferred `any`: a change to what the backend
    // pushes has to fail here rather than silently in a test's fake event.
    api.onDraftUpdated.mockImplementation((handler: (view: DraftView) => void) => {
      push.draft = handler;
      return Promise.resolve(() => undefined);
    });
    api.onPollHealth.mockImplementation((handler: (health: PollHealth) => void) => {
      push.health = handler;
      return Promise.resolve(() => undefined);
    });
    api.onSeasonUpdated.mockImplementation((handler: (view: SeasonView) => void) => {
      push.season = handler;
      return Promise.resolve(() => undefined);
    });
    api.onSeasonPollHealth.mockImplementation((handler: (health: PollHealth) => void) => {
      push.seasonHealth = handler;
      return Promise.resolve(() => undefined);
    });
    api.onSharedChat.mockImplementation((handler: (thread: SharedChatThread) => void) => {
      push.sharedChat = handler;
      return Promise.resolve(() => undefined);
    });
    api.onCompanionDevices.mockImplementation((handler: (devices: CompanionDevice[]) => void) => {
      push.devices = handler;
      return Promise.resolve(() => undefined);
    });
  };

  reset();
  return { api, push, reset };
}

let singleton: Harness | null = null;

export function harness(): Harness {
  singleton ??= build();
  return singleton;
}

/**
 * The committed dev fixture, deep-copied so a test can edit it.
 *
 * `available` is trimmed by default: the real fixture carries 393 players and
 * the board commits 200 rows of them, which is the single most expensive thing
 * in these tests and irrelevant to anything the shell itself does.
 */
export function draftFixture(availablePlayers = 3): DraftView {
  const view = structuredClone(draftFixtureJson) as unknown as DraftView;
  view.available = view.available.slice(0, availablePlayers);
  return view;
}

/** A config that restores the fixture league on launch. */
export function restoringConfig(view: DraftView): AppConfig {
  return {
    my_user_id: "browser-preview",
    active_league_id: view.league.league_id,
    leagues: [],
  };
}

/** The companion server as the harness reports it: off, with a code already
 *  minted, and nothing paired. */
export function companionStatus(overrides: Partial<CompanionStatus> = {}): CompanionStatus {
  return {
    enabled: false,
    url: "http://192.168.1.24:7878/",
    tailscale_url: null,
    code: "418902",
    port: 7878,
    host_name: "Justin's Mac",
    devices: [],
    ...overrides,
  };
}

/** The backend's diagnostics as the harness reports them: a healthy desktop
 *  app with a league open and a couple of lines in its log. */
export function diagnostics(overrides: Partial<Diagnostics> = {}): Diagnostics {
  return {
    app_version: "0.2.0",
    platform: "macos aarch64",
    league_id: "1",
    league_name: "Dynasty Warriors",
    draft_id: "d1",
    platform_name: "sleeper",
    polling: true,
    poll: { last_success_at: 1788452521, consecutive_failures: 0, last_error: null },
    companion_enabled: false,
    companion_devices: 0,
    log_path: "/Users/x/Library/Application Support/draft-assistant/draft-assistant.log",
    log_tail: ["2026-09-03T16:22:01Z INFO polling started every 3s"],
    ...overrides,
  };
}

/** A storage the test controls, since this jsdom has none of its own. */
export function fakeStorage(initial: Record<string, string> = {}): Map<string, string> {
  const store = new Map(Object.entries(initial));
  vi.stubGlobal("localStorage", {
    getItem: (key: string) => store.get(key) ?? null,
    setItem: (key: string, value: string) => void store.set(key, value),
    removeItem: (key: string) => void store.delete(key),
    clear: () => store.clear(),
  });
  return store;
}

/** A season view with one standings row, the user's own. */
export function seasonFixture(overrides: Partial<SeasonView> = {}): SeasonView {
  return {
    schema_version: "1.3",
    generated_at: 0,
    team_avatars: {},
    league: {
      league_id: "1",
      name: "Dynasty Warriors",
      season: "2026",
      platform: "sleeper",
      total_rosters: 12,
      roster_positions: ["QB", "RB", "BN"],
      draftable_positions: ["QB", "RB"],
      scoring_settings: {},
    },
    week: 3,
    season: "2026",
    my_roster_id: 1,
    header: {
      opponent_name: "punt_god",
      my_projected: 122.4,
      my_set_projected: 118.1,
      opp_projected: 108.9,
      win_odds_best: 0.62,
      win_odds_set: 0.55,
      playoff_odds: 0.88,
      locks_in_ms: null,
    },
    matchup: null,
    calls: [],
    points_on_table: 0,
    waivers: [],
    waiver_budget_left: 38,
    waiver_budget_total: 100,
    standings: [
      {
        roster_id: 1,
        seed: 2,
        name: "You",
        record: "2–0",
        wins: 2,
        losses: 0,
        ties: 0,
        points_for: 250,
        projected_points: 1642,
        playoff_odds: 0.88,
        is_mine: true,
      },
    ],
    live: {
      games: [],
      windows: [],
      totals: { my_playing: 0, my_pre: 0, my_done: 0, my_live_points: 0, opp_live_points: 0 },
      next_kickoff_ms: null,
      bye_teams: [],
    },
    roster: [],
    trades: [],
    recent_trades: [],
    activity: [],
    last_season: [],
    trends: { series: [], changes: [] },
    data_health: { fetched_at: 0, warnings: [] },
    ...overrides,
  };
}
