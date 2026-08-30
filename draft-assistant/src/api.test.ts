import { afterEach, beforeEach, describe, expect, it, vi } from "vitest";
import fixtureJson from "../public/dev-fixture.json";
import type { DraftView } from "./types";

/**
 * `api.ts` picks its implementation at import time — the Tauri bridge inside
 * the shell, the fixture/replay/recording preview in a browser tab — so each
 * describe resets modules and imports it fresh under the environment it
 * wants.
 */

const view = (): DraftView => structuredClone(fixtureJson as unknown as DraftView);

describe("DraftView schema guard", () => {
  it("accepts the current schema and rejects stale data with an actionable message", async () => {
    const { validateDraftView } = await import("./api");
    const current = { schema_version: "1.6" } as DraftView;
    expect(validateDraftView(current)).toBe(current);
    expect(() => validateDraftView({ schema_version: "1.2" } as DraftView)).toThrow(
      "expected schema 1.6, received 1.2",
    );
    expect(() => validateDraftView({} as DraftView)).toThrow("received missing");
  });
});

function jsonResponse(body: unknown, ok = true, status = 200): Response {
  return {
    ok,
    status,
    json: async () => body,
  } as Response;
}

afterEach(() => {
  vi.unstubAllGlobals();
  vi.useRealTimers();
  vi.resetModules();
  delete (window as unknown as Record<string, unknown>).__TAURI_INTERNALS__;
  window.history.replaceState({}, "", "/");
});

describe("browser preview", () => {
  it("serves the dev fixture and refuses to mutate", async () => {
    const fetchMock = vi.fn(async () => jsonResponse(view()));
    vi.stubGlobal("fetch", fetchMock);
    const { api } = await import("./api");
    expect(api.preview).toBe(true);
    expect(api.replay).toBeNull();

    const state = await api.getState();
    expect(state.league.league_id).toBe(fixtureJson.league.league_id);
    expect(fetchMock).toHaveBeenCalledWith("/dev-fixture.json", { cache: "no-store" });
    // Cached after the first read.
    await api.addLeague("anything");
    await api.refreshPicks();
    expect(fetchMock).toHaveBeenCalledTimes(1);

    const config = await api.getConfig();
    expect(config.active_league_id).toBe(fixtureJson.league.league_id);
    expect(config.leagues[0].name).toBe(fixtureJson.league.name);
    expect(await api.setMyUsername("me")).toBe("me");

    await expect(api.recordManualPick("x")).rejects.toThrow(/read-only/);
    await expect(api.undoManualPick()).rejects.toThrow(/read-only/);
    await expect(api.exportState()).rejects.toThrow(/read-only/);
    await expect(api.refreshData()).rejects.toThrow(/read-only/);
    await expect(api.startPolling()).rejects.toThrow(/read-only/);
    await expect(api.chat("q", [], { model: "opus", effort: null, fast: false, web_search: false })).rejects.toThrow(
      /desktop app/,
    );
    await expect(api.chatCompact([], { model: "opus", effort: null, fast: false, web_search: false })).rejects.toThrow(
      /desktop app/,
    );
  });

  it("explains a missing fixture, a bad replay source and a non-dump payload", async () => {
    vi.stubGlobal("fetch", vi.fn(async () => jsonResponse({}, false, 404)));
    let { api } = await import("./api");
    await expect(api.getState()).rejects.toThrow(/dev fixture missing/);

    vi.resetModules();
    window.history.replaceState({}, "", "/?replay=/live.json");
    vi.stubGlobal("fetch", vi.fn(async () => jsonResponse({}, false, 500)));
    ({ api } = await import("./api"));
    await expect(api.getState()).rejects.toThrow(/replay source \/live.json returned 500/);

    vi.resetModules();
    vi.stubGlobal(
      "fetch",
      vi.fn(
        async () =>
          ({
            ok: true,
            status: 200,
            json: async () => {
              throw new SyntaxError(`Unexpected token '<', "<!doctype "... is not valid JSON`);
            },
          }) as unknown as Response,
      ),
    );
    ({ api } = await import("./api"));
    await expect(api.getState()).rejects.toThrow(/could not read draft state from \/live.json/);
    await expect(api.getState()).rejects.not.toThrow(/Unexpected token/);

    vi.resetModules();
    vi.stubGlobal("fetch", vi.fn(async () => jsonResponse({ ...view(), schema_version: "0.9" })));
    ({ api } = await import("./api"));
    await expect(api.getState()).rejects.toThrow(/expected schema 1.6, received 0.9/);
  });

  it("replay mode polls the source and pushes only newer dumps", async () => {
    vi.useFakeTimers();
    window.history.replaceState({}, "", "/?replay=/live.json");
    const first = view();
    first.generated_at = 100;
    const responses = [first];
    vi.stubGlobal("fetch", vi.fn(async () => jsonResponse(responses[responses.length - 1])));
    const { api } = await import("./api");
    expect(api.replay).toBe("/live.json");
    await api.getState();

    const views: DraftView[] = [];
    const healths: unknown[] = [];
    const offView = await api.onDraftUpdated((v) => views.push(v));
    const offHealth = await api.onPollHealth((h) => healths.push(h));
    await api.startPolling();

    // Same dump again: nothing pushed.
    await vi.advanceTimersByTimeAsync(3100);
    expect(views).toHaveLength(0);

    // A newer dump: pushed, with seq taken from generated_at.
    const second = view();
    second.generated_at = 200;
    responses.push(second);
    await vi.advanceTimersByTimeAsync(3100);
    expect(views).toHaveLength(1);
    expect(views[0].seq).toBe(200);
    expect(healths).toHaveLength(1);
    expect(healths[0]).toMatchObject({ last_success_at: 200, consecutive_failures: 0, last_error: null });

    // An older dump is ignored; a half-written one is skipped silently.
    const older = view();
    older.generated_at = 150;
    responses.push(older);
    await vi.advanceTimersByTimeAsync(3100);
    responses.push({} as DraftView);
    await vi.advanceTimersByTimeAsync(3100);
    expect(views).toHaveLength(1);

    // Refresh re-reads the source; unsubscribing and stopping are clean.
    const fourth = view();
    fourth.generated_at = 250;
    responses.push(fourth);
    expect((await api.refreshData()).generated_at).toBe(250);
    offView();
    offHealth();
    await api.stopPolling();
    const third = view();
    third.generated_at = 300;
    responses.push(third);
    await vi.advanceTimersByTimeAsync(6200);
    expect(views).toHaveLength(1);
  });

  it("plays a recorded chat session back, streaming each answer", async () => {
    vi.useFakeTimers();
    window.history.replaceState({}, "", "/?chat=/session.json");
    const recording = [
      {
        question: "Who?",
        answer: "Take **Gibbs** now.",
        usage: { model: "opus", context_tokens: 100, duration_ms: 900, cost_usd: 0.1, web_searches: 0 },
        as_of: { pick: 2, seq: 5 },
      },
    ];
    const fetchMock = vi.fn(async (url: string) =>
      url === "/session.json" ? jsonResponse(recording) : jsonResponse(view()),
    );
    vi.stubGlobal("fetch", fetchMock);
    const { api } = await import("./api");
    expect(api.chatRecording).toBe("/session.json");

    const pieces: string[] = [];
    const options = { model: "opus", effort: null, fast: false, web_search: false };
    const pending = api.chat("Who?", [], options, (t) => pieces.push(t));
    await vi.advanceTimersByTimeAsync(200);
    const reply = await pending;
    expect(pieces.join("")).toBe("Take **Gibbs** now.");
    expect(pieces.length).toBeGreaterThan(1);
    expect(reply.answer).toBe("Take **Gibbs** now.");
    expect(reply.as_of).toEqual({ pick: 2, seq: 5 });

    await expect(api.chat("Why?", [], options)).rejects.toThrow(/no more answers/);
    expect(fetchMock).toHaveBeenCalledTimes(1);

    vi.resetModules();
    window.history.replaceState({}, "", "/?chat=/missing.json");
    vi.stubGlobal("fetch", vi.fn(async () => jsonResponse({}, false, 404)));
    const fresh = (await import("./api")).api;
    await expect(fresh.chat("Who?", [], options)).rejects.toThrow(/chat recording \/missing.json returned 404/);
  });
});

describe("Tauri bridge", () => {
  const invoke = vi.fn();
  const listen = vi.fn();
  class FakeChannel {
    onmessage: (text: string) => void = () => undefined;
  }

  beforeEach(() => {
    invoke.mockReset();
    listen.mockReset();
    (window as unknown as Record<string, unknown>).__TAURI_INTERNALS__ = {};
    vi.doMock("@tauri-apps/api/core", () => ({ invoke, Channel: FakeChannel }));
    vi.doMock("@tauri-apps/api/event", () => ({ listen }));
  });

  it("forwards every command with its arguments and validates views", async () => {
    const { api } = await import("./api");
    expect(api.preview).toBe(false);
    invoke.mockResolvedValue(view());

    await api.addLeague("123", true);
    expect(invoke).toHaveBeenLastCalledWith("add_league", { leagueId: "123", force: true });
    const lastCommand = () => invoke.mock.lastCall?.[0];
    await api.refreshPicks();
    expect(lastCommand()).toBe("refresh_picks");
    await api.refreshData();
    expect(lastCommand()).toBe("refresh_data");
    await api.getState();
    expect(lastCommand()).toBe("get_state");
    await api.recordManualPick("p1");
    expect(invoke).toHaveBeenLastCalledWith("record_manual_pick", { playerId: "p1" });
    await api.undoManualPick();
    expect(lastCommand()).toBe("undo_manual_pick");
    // Picks ride the same command as the players: the argument names here
    // are the contract with `desktop::evaluate_trade`, which nothing but the
    // desktop app can exercise.
    await api.evaluateTrade(4, ["p1"], ["p2"], [3], [1]);
    expect(invoke).toHaveBeenLastCalledWith("evaluate_trade", {
      partnerSlot: 4,
      give: ["p1"],
      get: ["p2"],
      givePicks: [3],
      getPicks: [1],
    });

    invoke.mockResolvedValueOnce("uid");
    expect(await api.setMyUsername("me")).toBe("uid");
    invoke.mockResolvedValueOnce({ my_user_id: null, active_league_id: null, leagues: [] });
    expect((await api.getConfig()).leagues).toEqual([]);
    invoke.mockResolvedValueOnce("/tmp/state.json");
    expect(await api.exportState()).toBe("/tmp/state.json");
    await api.startPolling(5);
    expect(invoke).toHaveBeenLastCalledWith("start_polling", { intervalSecs: 5 });
    await api.startPolling();
    expect(invoke).toHaveBeenLastCalledWith("start_polling", { intervalSecs: 3 });
    await api.stopPolling();
    expect(lastCommand()).toBe("stop_polling");

    invoke.mockResolvedValueOnce({ ...view(), schema_version: "2.0" });
    await expect(api.getState()).rejects.toThrow(/expected schema 1.6, received 2.0/);
  });

  it("streams chat text over a channel and passes compaction through", async () => {
    const { api } = await import("./api");
    const reply = { answer: "Gibbs.", usage: {}, as_of: null };
    invoke.mockImplementation(async (_cmd: string, args: { onText?: FakeChannel }) => {
      args.onText?.onmessage("Gib");
      args.onText?.onmessage("bs.");
      return reply;
    });
    const pieces: string[] = [];
    const options = { model: "opus", effort: null, fast: false, web_search: false };
    const result = await api.chat("Who?", [{ role: "you", text: "hi" }], options, (t) => pieces.push(t));
    expect(pieces).toEqual(["Gib", "bs."]);
    expect(result).toBe(reply);
    expect(invoke).toHaveBeenCalledWith(
      "chat",
      expect.objectContaining({ question: "Who?", history: [{ role: "you", text: "hi" }], options }),
    );
    // No callback: the channel's messages are simply dropped.
    await api.chat("Who?", [], options);

    invoke.mockResolvedValueOnce(reply);
    await api.chatCompact([], options);
    expect(invoke).toHaveBeenLastCalledWith("chat_compact", { history: [], options });
  });

  it("validates live updates and reports rejected payloads instead of dropping them", async () => {
    const { api } = await import("./api");
    let handler: (event: { payload: unknown }) => void = () => undefined;
    const unlisten = vi.fn();
    listen.mockImplementation(async (_name: string, cb: typeof handler) => {
      handler = cb;
      return unlisten;
    });
    const views: DraftView[] = [];
    const errors: unknown[] = [];
    const off = await api.onDraftUpdated((v) => views.push(v), (e) => errors.push(e));
    expect(listen).toHaveBeenCalledWith("draft-updated", expect.any(Function));

    handler({ payload: view() });
    expect(views).toHaveLength(1);
    handler({ payload: { ...view(), schema_version: "9" } });
    expect(views).toHaveLength(1);
    expect(errors).toHaveLength(1);
    expect(String(errors[0])).toMatch(/expected schema 1.6, received 9/);
    off();
    expect(unlisten).toHaveBeenCalled();

    const healths: unknown[] = [];
    await api.onPollHealth((h) => healths.push(h));
    expect(listen).toHaveBeenLastCalledWith("poll-health", expect.any(Function));
    handler({ payload: { last_success_at: 1, consecutive_failures: 0, last_error: null } });
    expect(healths).toHaveLength(1);
  });
});
