// The api module picks its backend at import time (Tauri shell vs browser
// fixtures), so each arm is loaded fresh with the environment it expects.

import { afterEach, describe, expect, it, vi } from "vitest";
import type { DraftView } from "./types";
import type { SeasonView } from "./season-types";

const invoke = vi.fn();
const listen = vi.fn();
vi.mock("@tauri-apps/api/core", () => ({ invoke: (...a: unknown[]) => invoke(...a) }));
vi.mock("@tauri-apps/api/event", () => ({ listen: (...a: unknown[]) => listen(...a) }));

const draftView = {
  schema_version: "1.1",
  league: { league_id: "L1", name: "Test", season: "2026" },
} as unknown as DraftView;
const seasonView = { schema_version: "1.0" } as unknown as SeasonView;

async function load(shell: boolean) {
  vi.resetModules();
  const w = window as unknown as Record<string, unknown>;
  if (shell) w.__TAURI_INTERNALS__ = {};
  else delete w.__TAURI_INTERNALS__;
  return import("./api");
}

afterEach(() => {
  invoke.mockReset();
  listen.mockReset();
  vi.unstubAllGlobals();
  delete (window as unknown as Record<string, unknown>).__TAURI_INTERNALS__;
});

describe("schema validation", () => {
  it("accepts matching versions and rejects mismatches with a readable error", async () => {
    const { validateDraftView, validateSeasonView } = await load(false);
    expect(validateDraftView(draftView)).toBe(draftView);
    expect(validateSeasonView(seasonView)).toBe(seasonView);
    expect(() => validateDraftView({ schema_version: "0.9" } as DraftView)).toThrow(
      /expected schema 1\.1, received 0\.9/,
    );
    expect(() => validateSeasonView({} as SeasonView)).toThrow(/received missing/);
  });
});

describe("tauri arm", () => {
  it("routes commands through invoke with their arguments", async () => {
    const { api } = await load(true);
    invoke.mockResolvedValue(draftView);
    await api.addLeague("L1", true);
    expect(invoke).toHaveBeenCalledWith("add_league", { leagueId: "L1", force: true });
    await api.recordManualPick("123");
    expect(invoke).toHaveBeenCalledWith("record_manual_pick", { playerId: "123" });
    await api.getState();
    await api.refreshPicks();
    await api.refreshData();
    await api.undoManualPick();

    invoke.mockResolvedValue(seasonView);
    await api.loadSeason();
    expect(invoke).toHaveBeenCalledWith("load_season", { force: false });
    await api.getSeason();
    await api.refreshSeason();

    invoke.mockResolvedValue(undefined);
    await api.startPolling();
    expect(invoke).toHaveBeenCalledWith("start_polling", { intervalSecs: 3 });
    await api.startSeasonPolling();
    expect(invoke).toHaveBeenCalledWith("start_season_polling", { intervalSecs: 30 });
    await api.stopPolling();
    await api.stopSeasonPolling();

    invoke.mockResolvedValue("chriswitz");
    await api.setMyUsername("chriswitz");
    expect(invoke).toHaveBeenCalledWith("set_my_username", { username: "chriswitz" });
    await api.exportState();
    invoke.mockResolvedValue(null);
    await api.headshot("123");
    expect(invoke).toHaveBeenCalledWith("headshot", { playerId: "123" });
    await api.avatar("abc123", true);
    expect(invoke).toHaveBeenCalledWith("avatar", { reference: "abc123", full: true });
    invoke.mockResolvedValue({});
    await api.getConfig();
    invoke.mockResolvedValue(true);
    await api.setApiKey("sk-test");
    invoke.mockResolvedValue("api");
    await api.setChatProvider("api");
    invoke.mockResolvedValue({ provider: "api" });
    await api.chatSettings();
    invoke.mockResolvedValue([]);
    await api.chatSuggestions("season");
    invoke.mockResolvedValue({ text: "hi" });
    await api.askClaude({ screen: "season", model: "Opus 5", effort: "Low", messages: [] });
    expect(invoke).toHaveBeenCalledWith("ask_claude", {
      screen: "season",
      model: "Opus 5",
      effort: "Low",
      messages: [],
    });
  });

  it("rejects a draft view with the wrong schema before it reaches the UI", async () => {
    const { api } = await load(true);
    invoke.mockResolvedValue({ schema_version: "9.9" });
    await expect(api.getState()).rejects.toThrow(/Incompatible draft data/);
  });

  it("validates event payloads before handing them to listeners", async () => {
    const { api } = await load(true);
    let deliver: ((event: { payload: unknown }) => void) | null = null;
    listen.mockImplementation(async (_name: string, cb: (event: { payload: unknown }) => void) => {
      deliver = cb;
      return () => undefined;
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
      vi.fn(async () => ({ ok, json: async () => body })),
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
    expect((await api.onDraftUpdated(() => undefined))()).toBeUndefined();
    expect((await api.onPollHealth(() => undefined))()).toBeUndefined();
    expect((await api.onSeasonUpdated(() => undefined))()).toBeUndefined();
    expect((await api.onSeasonPollHealth(() => undefined))()).toBeUndefined();
  });
});
