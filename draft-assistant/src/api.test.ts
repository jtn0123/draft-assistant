import { afterEach, beforeEach, describe, expect, it, vi } from "vitest";
import type { DraftView } from "./types";
import { validateDraftView } from "./api";

describe("DraftView schema guard", () => {
  it("accepts the current schema", () => {
    const view = { schema_version: "1.3" } as DraftView;
    expect(validateDraftView(view)).toBe(view);
  });

  it("rejects stale data with an actionable message", () => {
    const view = { schema_version: "1.2" } as DraftView;
    expect(() => validateDraftView(view)).toThrow(
      "expected schema 1.3, received 1.2",
    );
  });
});

describe("browser preview replay mode", () => {
  const dump = (generatedAt: number): DraftView =>
    ({ schema_version: "1.3", seq: 1, generated_at: generatedAt }) as DraftView;

  beforeEach(() => {
    vi.useFakeTimers();
    window.history.pushState({}, "", "/?replay=/live-state.json");
    vi.resetModules();
  });

  afterEach(() => {
    vi.useRealTimers();
    vi.unstubAllGlobals();
    window.history.pushState({}, "", "/");
  });

  it("polls the replay URL and forwards only newer dumps, ordered by generated_at", async () => {
    const fetchMock = vi
      .fn()
      .mockResolvedValueOnce({ ok: true, json: async () => dump(100) }) // initial load
      .mockResolvedValueOnce({ ok: true, json: async () => dump(100) }) // poll: unchanged
      .mockResolvedValueOnce({ ok: true, json: async () => dump(104) }) // poll: newer
      .mockResolvedValueOnce({ ok: true, json: async () => dump(104) }); // poll: unchanged
    vi.stubGlobal("fetch", fetchMock);

    const { api } = await import("./api");
    expect(api.preview).toBe(true);
    expect(api.replay).toBe("/live-state.json");
    expect((await api.getState()).generated_at).toBe(100);
    expect(fetchMock).toHaveBeenLastCalledWith("/live-state.json", { cache: "no-store" });

    const views: DraftView[] = [];
    await api.onDraftUpdated((view) => views.push(view));
    await api.startPolling(3);

    await vi.advanceTimersByTimeAsync(3000);
    expect(views).toHaveLength(0);
    await vi.advanceTimersByTimeAsync(3000);
    expect(views.map((v) => v.seq)).toEqual([104]);
    await vi.advanceTimersByTimeAsync(3000);
    expect(views).toHaveLength(1);

    await api.stopPolling();
    await vi.advanceTimersByTimeAsync(9000);
    expect(fetchMock).toHaveBeenCalledTimes(4);
  });

  // Dogfood ISSUE-002/003: both reported success in a preview that cannot
  // refresh or write anything.
  it("refuses export and data refresh in the fixture preview", async () => {
    window.history.pushState({}, "", "/");
    vi.resetModules();
    const fetchMock = vi.fn().mockResolvedValue({ ok: true, json: async () => dump(1) });
    vi.stubGlobal("fetch", fetchMock);
    const { api } = await import("./api");
    await expect(api.exportState()).rejects.toThrow(/read-only/);
    await expect(api.refreshData()).rejects.toThrow(/read-only/);
  });

  it("reloads the replay dump when asked to refresh", async () => {
    const fetchMock = vi
      .fn()
      .mockResolvedValueOnce({ ok: true, json: async () => dump(100) })
      .mockResolvedValueOnce({ ok: true, json: async () => dump(140) });
    vi.stubGlobal("fetch", fetchMock);
    const { api } = await import("./api");
    expect((await api.getState()).generated_at).toBe(100);
    expect((await api.refreshData()).generated_at).toBe(140);
  });

  // Dogfood ISSUE-005: a source that is not JSON surfaced the raw
  // "Unexpected token '<'" parser message on the Setup screen.
  it("explains a state source that is not draft state", async () => {
    const fetchMock = vi.fn().mockResolvedValue({
      ok: true,
      json: async () => {
        throw new SyntaxError(`Unexpected token '<', "<!doctype "... is not valid JSON`);
      },
    });
    vi.stubGlobal("fetch", fetchMock);
    const { api } = await import("./api");
    await expect(api.getState()).rejects.toThrow(
      /could not read draft state from \/live-state\.json/,
    );
    await expect(api.getState()).rejects.not.toThrow(/Unexpected token/);
  });

  it("still refuses live sync when no replay source is given", async () => {
    window.history.pushState({}, "", "/");
    vi.resetModules();
    const { api } = await import("./api");
    expect(api.replay).toBeNull();
    await expect(api.startPolling()).rejects.toThrow(/read-only/);
  });
});
