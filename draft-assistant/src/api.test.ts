// The api module picks its backend at import time (Tauri shell vs browser
// fixtures), so each arm is loaded fresh with the environment it expects.

import { afterEach, describe, expect, it, vi } from "vitest";
import type { Api } from "./api";
import type { DraftView } from "./types";
import type { SeasonView } from "./season-types";
import { DRAFT_SCHEMA_VERSION } from "./draft-contract.generated";

const invoke = vi.fn();
const listen = vi.fn();
vi.mock("@tauri-apps/api/core", () => ({
  invoke: (...a: unknown[]): unknown => invoke(...a) as unknown,
}));
vi.mock("@tauri-apps/api/event", () => ({
  listen: (...a: unknown[]): unknown => listen(...a) as unknown,
}));

const draftView = {
  schema_version: DRAFT_SCHEMA_VERSION,
  league: { league_id: "L1", name: "Test", season: "2026", platform: "sleeper" },
} as unknown as DraftView;
const seasonView = { schema_version: "1.4" } as unknown as SeasonView;

async function load(shell: boolean, search = "") {
  vi.resetModules();
  const w = window as unknown as Record<string, unknown>;
  if (shell) w.__TAURI_INTERNALS__ = {};
  else delete w.__TAURI_INTERNALS__;
  // The browser arm reads the query string once, as the module is created.
  window.history.replaceState(null, "", `/${search}`);
  return import("./api");
}

afterEach(() => {
  invoke.mockReset();
  listen.mockReset();
  vi.unstubAllGlobals();
  vi.useRealTimers();
  window.history.replaceState(null, "", "/");
  delete (window as unknown as Record<string, unknown>).__TAURI_INTERNALS__;
});

describe("schema validation", () => {
  it("accepts matching versions and rejects mismatches with a readable error", async () => {
    const { validateDraftView, validateSeasonView } = await load(false);
    expect(validateDraftView(draftView)).toBe(draftView);
    expect(validateSeasonView(seasonView)).toBe(seasonView);
    expect(() => validateDraftView({ schema_version: "0.9" } as DraftView)).toThrow(
      `expected schema ${DRAFT_SCHEMA_VERSION}, received 0.9`,
    );
    expect(() => validateSeasonView({} as SeasonView)).toThrow(/received missing/);
  });
});

/**
 * Every command the desktop arm can send, as [the call, the reply it needs,
 * the invoke it must make]. Kept as data so each row names both halves of the
 * contract: the Rust command name and the exact argument payload. A call that
 * is pointed at the wrong command, or that drops or renames an argument, fails
 * on its own row.
 */
type Route = [(a: Api) => Promise<unknown>, unknown, unknown[]];
const ROUTES: Route[] = [
  [(a) => a.addLeague("L1", true), draftView, ["add_league", { leagueId: "L1", force: true }]],
  [(a) => a.addLeague("L2"), draftView, ["add_league", { leagueId: "L2", force: false }]],
  [
    (a) => a.setMyUsername("chriswitz"),
    "chriswitz",
    ["set_my_username", { username: "chriswitz" }],
  ],
  [(a) => a.getConfig(), {}, ["get_config"]],
  [(a) => a.sleeperLeagues("2026"), [], ["sleeper_leagues", { season: "2026" }]],
  [(a) => a.removeLeague("L1"), [], ["remove_league", { leagueId: "L1" }]],
  [(a) => a.yahooStatus(), {}, ["yahoo_status"]],
  [
    (a) => a.yahooSaveCredentials("dj0yJm", "shh"),
    {},
    ["yahoo_save_credentials", { clientId: "dj0yJm", clientSecret: "shh" }],
  ],
  [(a) => a.yahooBeginConnect(), {}, ["yahoo_begin_connect"]],
  [
    (a) => a.yahooFinishConnect("xy7q9", "s-1"),
    {},
    ["yahoo_finish_connect", { code: "xy7q9", state: "s-1" }],
  ],
  [(a) => a.yahooCancelConnect(), undefined, ["yahoo_cancel_connect"]],
  [(a) => a.yahooDisconnect(), {}, ["yahoo_disconnect", { forgetCredentials: false }]],
  [(a) => a.yahooDisconnect(true), {}, ["yahoo_disconnect", { forgetCredentials: true }]],
  [(a) => a.yahooLeagues(), [], ["yahoo_leagues"]],
  [(a) => a.getState(), draftView, ["get_state", undefined]],
  [(a) => a.refreshPicks(), draftView, ["refresh_picks", undefined]],
  [(a) => a.refreshData(), draftView, ["refresh_data", undefined]],
  [(a) => a.recordManualPick("123"), draftView, ["record_manual_pick", { playerId: "123" }]],
  [(a) => a.undoManualPick(), draftView, ["undo_manual_pick", undefined]],
  [(a) => a.clearKeepers(), draftView, ["clear_keepers", undefined]],
  [(a) => a.exportState(), "/tmp/draft.json", ["export_state"]],
  [(a) => a.importSecondOpinion(), null, ["import_second_opinion"]],
  [(a) => a.headshot("123"), null, ["headshot", { playerId: "123" }]],
  [(a) => a.avatar("abc123", true), null, ["avatar", { reference: "abc123", full: true }]],
  [(a) => a.startPolling(), undefined, ["start_polling", { intervalSecs: 3 }]],
  [(a) => a.startPolling(7), undefined, ["start_polling", { intervalSecs: 7 }]],
  [(a) => a.stopPolling(), undefined, ["stop_polling"]],
  [(a) => a.loadSeason(), seasonView, ["load_season", { force: false }]],
  [(a) => a.loadSeason(true), seasonView, ["load_season", { force: true }]],
  [(a) => a.getSeason(), seasonView, ["get_season", undefined]],
  [(a) => a.refreshSeason(), seasonView, ["refresh_season", undefined]],
  [(a) => a.startSeasonPolling(), undefined, ["start_season_polling", { intervalSecs: 30 }]],
  [(a) => a.startSeasonPolling(9), undefined, ["start_season_polling", { intervalSecs: 9 }]],
  [(a) => a.stopSeasonPolling(), undefined, ["stop_season_polling"]],
  [(a) => a.setApiKey("sk-test"), true, ["set_api_key", { key: "sk-test" }]],
  [(a) => a.setChatProvider("api"), "api", ["set_chat_provider", { provider: "api" }]],
  [(a) => a.setChatBudget(5), 5, ["set_chat_budget", { dollars: 5 }]],
  [(a) => a.chatSettings(), { provider: "api" }, ["chat_settings"]],
  [(a) => a.chatSuggestions("season"), [], ["chat_suggestions", { screen: "season" }]],
  [
    (a) => a.askClaude({ screen: "season", model: "Opus 5", effort: "Low", messages: [] }),
    { text: "hi" },
    ["ask_claude", { screen: "season", model: "Opus 5", effort: "Low", messages: [] }],
  ],
  [(a) => a.companionStatus(), {}, ["companion_status"]],
  [(a) => a.companionEnable(), {}, ["companion_enable"]],
  [(a) => a.companionDisable(), {}, ["companion_disable"]],
  [(a) => a.companionRevoke(), {}, ["companion_revoke"]],
  [(a) => a.setDeviceName("Mac"), "Mac", ["set_device_name", { name: "Mac" }]],
  [(a) => a.sharedChatGet("draft"), {}, ["shared_chat_get", { screen: "draft" }]],
  [
    (a) => a.sharedChatSend("draft", "who is left"),
    undefined,
    ["shared_chat_send", { screen: "draft", text: "who is left" }],
  ],
  [(a) => a.sharedChatReset("draft"), undefined, ["shared_chat_reset", { screen: "draft" }]],
  [(a) => a.diagnostics(), {}, ["diagnostics"]],
  [(a) => a.openLogFolder(), "/tmp/logs", ["open_log_folder"]],
  [
    (a) => a.logFrontendError("boom", "render", "at x"),
    undefined,
    ["log_frontend_error", { message: "boom", source: "render", stack: "at x" }],
  ],
  [(a) => a.setLogLevel("debug"), "debug", ["set_log_level", { level: "debug" }]],
  [(a) => a.checkForUpdate?.() ?? Promise.resolve(), {}, ["check_for_update"]],
  [(a) => a.installUpdate?.() ?? Promise.resolve(), undefined, ["install_update"]],
];

describe("tauri arm", () => {
  it("routes commands through invoke with their arguments", async () => {
    const { api } = await load(true);
    for (const [run, reply, expected] of ROUTES) {
      invoke.mockReset();
      invoke.mockResolvedValue(reply);
      await run(api);
      // The message names the row, so a failure says which command moved.
      expect(invoke.mock.calls, `api call expected to invoke ${String(expected[0])}`).toEqual([
        expected,
      ]);
    }
  });

  it("rejects a draft view with the wrong schema before it reaches the UI", async () => {
    const { api } = await load(true);
    invoke.mockResolvedValue({ schema_version: "9.9" });
    await expect(api.getState()).rejects.toThrow(/Incompatible draft data/);
  });

  it("validates event payloads before handing them to listeners", async () => {
    const { api } = await load(true);
    let deliver: ((event: { payload: unknown }) => void) | null = null;
    listen.mockImplementation((_name: string, cb: (event: { payload: unknown }) => void) => {
      deliver = cb;
      return Promise.resolve(() => undefined);
    });
    const seen: unknown[] = [];
    await api.onDraftUpdated((v) => seen.push(v));
    deliver!({ payload: draftView });
    expect(seen).toEqual([draftView]);
    expect(() => deliver!({ payload: { schema_version: "0.1" } })).toThrow(/Incompatible/);

    await api.onSeasonUpdated((v) => seen.push(v));
    deliver!({ payload: seasonView });
    expect(seen).toContain(seasonView);

    await api.onPollHealth((h) => seen.push(h));
    deliver!({ payload: { ok: true } });
    expect(seen[seen.length - 1]).toEqual({ ok: true });

    // Season health is passed through as-is: it carries no schema stamp, so
    // there is nothing to validate and nothing to throw away.
    const health = { last_success_at: 1, consecutive_failures: 2, last_error: "down" };
    await api.onSeasonPollHealth((h) => seen.push(h));
    deliver!({ payload: health });
    expect(seen[seen.length - 1]).toEqual(health);
  });
});

describe("browser arm", () => {
  const fixtureFetch = (body: unknown, ok = true) =>
    vi.stubGlobal(
      "fetch",
      vi.fn(() => Promise.resolve({ ok, json: () => Promise.resolve(body) })),
    );

  it("serves and caches the draft fixture", async () => {
    const { api } = await load(false);
    fixtureFetch(draftView);
    const first = await api.getState();
    const again = await api.refreshPicks();
    expect(again).toBe(first);
    expect(fetch).toHaveBeenCalledTimes(1);
    const config = await api.getConfig();
    expect(config.leagues[0]?.league_id).toBe("L1");
    expect(config.leagues[0]?.platform).toBe("sleeper");
    expect(config.my_user_id).toBe("browser-preview");
  });

  it("serves and caches the season fixture", async () => {
    const { api } = await load(false);
    fixtureFetch(seasonView);
    const first = await api.loadSeason();
    expect(await api.getSeason()).toBe(first);
    expect(await api.refreshSeason()).toBe(first);
    expect(fetch).toHaveBeenCalledTimes(1);
  });

  it("explains a missing fixture instead of failing cryptically", async () => {
    const { api } = await load(false);
    fixtureFetch(null, false);
    await expect(api.getState()).rejects.toThrow(/dev fixture missing/);
    await expect(api.getSeason()).rejects.toThrow(/season fixture missing/);
  });

  it("builds Sleeper CDN URLs for headshots and avatars", async () => {
    const { api } = await load(false);
    expect(await api.headshot("4881")).toBe(
      "https://sleepercdn.com/content/nfl/players/thumb/4881.jpg",
    );
    expect(await api.headshot("JAX")).toBeNull();
    expect(await api.avatar("abc123", false)).toBe("https://sleepercdn.com/avatars/thumbs/abc123");
    expect(await api.avatar("abc123", true)).toBe("https://sleepercdn.com/avatars/abc123");
    expect(await api.avatar("https://sleepercdn.com/uploads/x.jpg", false)).toBe(
      "https://sleepercdn.com/uploads/x.jpg",
    );
    expect(await api.avatar("not hex!", false)).toBeNull();
  });

  it("refuses mutations and live features, and no-ops the safe calls", async () => {
    const { api } = await load(false);
    await expect(api.recordManualPick("1")).rejects.toThrow(/read-only/);
    await expect(api.undoManualPick()).rejects.toThrow(/read-only/);
    await expect(api.startPolling()).rejects.toThrow(/read-only/);
    await expect(api.startSeasonPolling()).rejects.toThrow(/read-only/);
    await expect(
      api.askClaude({ screen: "s", model: "m", effort: "e", messages: [] }),
    ).rejects.toThrow(/read-only/);
    expect(await api.setMyUsername("me")).toBe("me");
    expect(await api.exportState()).toMatch(/no export/);
    expect(await api.setApiKey("k")).toBe(false);
    expect(await api.setChatProvider("api")).toBe("api");
    expect(await api.chatSuggestions("draft")).toEqual([]);
    expect((await api.chatSettings()).has_key).toBe(false);
    await api.stopPolling();
    await api.stopSeasonPolling();
    // Nothing can arrive on a fixture that never moves, so what an
    // unsubscriber actually does is asserted in the replay arm below, where a
    // dump can be pushed after it has been called.
  });

  it("says Yahoo needs the desktop app, and reports nothing configured", async () => {
    const { api } = await load(false);
    expect(await api.yahooStatus()).toEqual({
      configured: false,
      connected: false,
      redirect: "oob",
      account: null,
    });
    await expect(api.yahooSaveCredentials("a", "b")).rejects.toThrow("Yahoo needs the desktop app");
    await expect(api.yahooBeginConnect()).rejects.toThrow("Yahoo needs the desktop app");
    await expect(api.yahooFinishConnect("c", "s")).rejects.toThrow("Yahoo needs the desktop app");
    await expect(api.yahooDisconnect()).rejects.toThrow("Yahoo needs the desktop app");
    await expect(api.yahooLeagues()).rejects.toThrow("Yahoo needs the desktop app");
    // Nothing can have begun in the browser, so giving up is not an error.
    await expect(api.yahooCancelConnect()).resolves.toBeUndefined();
  });

  it("calls a fixture with no platform on it a Sleeper league", async () => {
    const { api } = await load(false);
    fixtureFetch({ ...draftView, league: { league_id: "L1", name: "Test", season: "2026" } });
    expect((await api.sleeperLeagues("2026"))[0]?.platform).toBe("sleeper");
  });
});

describe("replay arm", () => {
  /** A fetch that answers each URL with the newest dump written for it. */
  const replayFetch = (dumps: Record<string, unknown>) =>
    vi.stubGlobal(
      "fetch",
      vi.fn((url: string) =>
        Promise.resolve({ ok: true, json: () => Promise.resolve(dumps[url]) }),
      ),
    );

  it("reads the draft state from ?replay and pushes newer dumps to the listeners", async () => {
    vi.useFakeTimers();
    const dumps: Record<string, unknown> = {
      "/live-state.json": { ...draftView, generated_at: 100 },
    };
    replayFetch(dumps);
    const { api } = await load(false, "?replay=/live-state.json");
    const seen: DraftView[] = [];
    const health: unknown[] = [];
    await api.onDraftUpdated((v) => seen.push(v));
    await api.onPollHealth((h) => health.push(h));

    expect((await api.getState()).generated_at).toBe(100);
    // Live sync is no longer refused: it is the replay timer.
    await expect(api.startPolling()).resolves.toBeUndefined();

    dumps["/live-state.json"] = { ...draftView, generated_at: 101 };
    await vi.advanceTimersByTimeAsync(3000);
    expect(seen.map((v) => v.generated_at)).toEqual([101]);
    expect(health).toEqual([{ last_success_at: 101, consecutive_failures: 0, last_error: null }]);

    await api.stopPolling();
    dumps["/live-state.json"] = { ...draftView, generated_at: 102 };
    await vi.advanceTimersByTimeAsync(6000);
    expect(seen).toHaveLength(1);
  });

  it("replays the season from its own parameter, leaving the draft on its fixture", async () => {
    vi.useFakeTimers();
    const dumps: Record<string, unknown> = {
      "/dev-fixture.json": draftView,
      "/live-season.json": { ...seasonView, generated_at: 5 },
    };
    replayFetch(dumps);
    const { api } = await load(false, "?replay-season=/live-season.json");
    const seen: SeasonView[] = [];
    await api.onSeasonUpdated((v) => seen.push(v));
    expect((await api.loadSeason()).generated_at).toBe(5);
    await expect(api.startSeasonPolling()).resolves.toBeUndefined();
    // The draft half was never pointed anywhere, so it stays read-only.
    await expect(api.startPolling()).rejects.toThrow(/read-only/);

    dumps["/live-season.json"] = { ...seasonView, generated_at: 6 };
    await vi.advanceTimersByTimeAsync(3000);
    expect(seen.map((v) => v.generated_at)).toEqual([6]);
    await api.stopSeasonPolling();
  });

  it("stops delivering to a listener once its unsubscriber is called", async () => {
    vi.useFakeTimers();
    const dumps: Record<string, unknown> = {
      "/live-state.json": { ...draftView, generated_at: 200 },
      "/live-season.json": { ...seasonView, generated_at: 200 },
    };
    replayFetch(dumps);
    const { api } = await load(false, "?replay=/live-state.json&replay-season=/live-season.json");
    const gone: string[] = [];
    const kept: string[] = [];
    const drop = [
      await api.onDraftUpdated(() => gone.push("draft")),
      await api.onPollHealth(() => gone.push("health")),
      await api.onSeasonUpdated(() => gone.push("season")),
      await api.onSeasonPollHealth(() => gone.push("season-health")),
    ];
    await api.onDraftUpdated(() => kept.push("draft"));
    await api.onPollHealth(() => kept.push("health"));
    await api.onSeasonUpdated(() => kept.push("season"));
    await api.onSeasonPollHealth(() => kept.push("season-health"));
    for (const unsubscribe of drop) unsubscribe();

    await api.startPolling();
    await api.startSeasonPolling();
    dumps["/live-state.json"] = { ...draftView, generated_at: 201 };
    dumps["/live-season.json"] = { ...seasonView, generated_at: 201 };
    await vi.advanceTimersByTimeAsync(3000);

    // The listeners that stayed prove a dump really was pushed, so an empty
    // `gone` means removal rather than a tick that never happened.
    expect(kept).toEqual(["draft", "health", "season", "season-health"]);
    expect(gone).toEqual([]);
    await api.stopPolling();
    await api.stopSeasonPolling();
  });

  it("says a replay source answered with a page rather than a dump", async () => {
    vi.stubGlobal(
      "fetch",
      vi.fn(() => Promise.resolve({ ok: true, json: () => Promise.reject(new SyntaxError("<")) })),
    );
    const { api } = await load(false, "?replay=/typo.json");
    await expect(api.getState()).rejects.toThrow(/it is not a state dump/);
  });
});
