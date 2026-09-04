// The follower's backend, driven against a fake host: a stubbed `fetch` for
// the reads and a fake WebSocket for the live half.

import { afterEach, beforeEach, describe, expect, it, vi } from "vitest";
import { remoteApi, remoteFetcher, REVOKED_KEY } from "./apiRemote";
import type { DraftView, FollowRecord } from "./types";

const follow: FollowRecord = {
  url: "http://192.168.1.5:7878",
  token: "tok-1",
  host_name: "Justin's Mac",
};

const draftView = {
  schema_version: "1.4",
  league: { league_id: "L1", name: "Test", season: "2026", platform: "sleeper" },
} as unknown as DraftView;

/** A stand-in for the browser's socket that a test can push frames into. */
class FakeSocket {
  static live: FakeSocket[] = [];
  onopen: (() => void) | null = null;
  onmessage: ((event: { data: string }) => void) | null = null;
  onclose: (() => void) | null = null;
  onerror: (() => void) | null = null;
  sent: string[] = [];
  closed = false;
  constructor(readonly url: string) {
    FakeSocket.live.push(this);
  }
  send(text: string): void {
    this.sent.push(text);
  }
  close(): void {
    this.closed = true;
    this.onclose?.();
  }
  /** What the host would push down the wire. */
  push(type: string, payload?: unknown): void {
    this.onmessage?.({ data: JSON.stringify({ type, payload }) });
  }
}

const newest = (): FakeSocket => {
  const socket = FakeSocket.live[FakeSocket.live.length - 1];
  if (socket === undefined) throw new Error("no socket was opened");
  return socket;
};

const fetchMock = vi.fn();

/** A JSON answer with a status. */
function json(body: unknown, status = 200): Response {
  return {
    ok: status >= 200 && status < 300,
    status,
    headers: new Headers({ "content-type": "application/json" }),
    json: () => Promise.resolve(body),
  } as unknown as Response;
}

beforeEach(() => {
  FakeSocket.live = [];
  fetchMock.mockReset();
  vi.stubGlobal("fetch", fetchMock);
  vi.stubGlobal("WebSocket", FakeSocket);
});

afterEach(() => {
  vi.unstubAllGlobals();
  vi.useRealTimers();
});

describe("reads", () => {
  it("gets the state from the host with the pairing token", async () => {
    fetchMock.mockResolvedValue(json(draftView));
    const view = await remoteApi(follow, () => undefined).getState();
    expect(view.league.league_id).toBe("L1");
    expect(fetchMock).toHaveBeenCalledWith("http://192.168.1.5:7878/api/state", {
      headers: { authorization: "Bearer tok-1" },
    });
  });

  it("reads a 404 as nothing there, and says so by name", async () => {
    fetchMock.mockResolvedValue(json({ error: "no league loaded" }, 404));
    await expect(remoteFetcher(follow, () => undefined)("/api/season")).resolves.toBeNull();
    await expect(remoteApi(follow, () => undefined).getSeason()).rejects.toThrow(
      /Justin's Mac hasn't opened the Season screen yet/,
    );
  });

  it("fills in what /api/config does not carry", async () => {
    fetchMock.mockResolvedValue(json({ host_name: "Justin's Mac", platform: "sleeper" }));
    const config = await remoteApi(follow, () => undefined).getConfig();
    expect(config).toEqual({ my_user_id: null, active_league_id: null, leagues: [] });
  });

  it("turns image bytes into a data URL", async () => {
    fetchMock.mockResolvedValue({
      ok: true,
      status: 200,
      headers: new Headers({ "content-type": "image/png" }),
      arrayBuffer: () => Promise.resolve(new Uint8Array([1, 2, 3]).buffer),
    });
    await expect(remoteApi(follow, () => undefined).headshot("4046")).resolves.toBe(
      `data:image/png;base64,${btoa("")}`,
    );
  });
});

describe("the shared socket", () => {
  it("delivers draft updates to whoever subscribed", async () => {
    const api = remoteApi(follow, () => undefined);
    const seen: DraftView[] = [];
    await api.onDraftUpdated((view) => seen.push(view));
    expect(newest().url).toBe("ws://192.168.1.5:7878/api/events?token=tok-1");
    newest().push("draft-updated", draftView);
    expect(seen).toHaveLength(1);
  });

  it("opens one socket for every subscription and reconnects when it drops", async () => {
    vi.useFakeTimers();
    const api = remoteApi(follow, () => undefined);
    await api.onDraftUpdated(() => undefined);
    await api.onPollHealth(() => undefined);
    expect(FakeSocket.live).toHaveLength(1);
    newest().close();
    await vi.advanceTimersByTimeAsync(1000);
    expect(FakeSocket.live).toHaveLength(2);
  });

  it("keeps the connection warm with a ping", async () => {
    vi.useFakeTimers();
    await remoteApi(follow, () => undefined).onDraftUpdated(() => undefined);
    newest().onopen?.();
    await vi.advanceTimersByTimeAsync(25000);
    expect(newest().sent).toEqual([JSON.stringify({ type: "ping" })]);
  });

  it("hands a revoked frame to the shell and stops reconnecting", async () => {
    vi.useFakeTimers();
    const revoked = vi.fn();
    await remoteApi(follow, revoked).onDraftUpdated(() => undefined);
    newest().push("revoked", {});
    expect(revoked).toHaveBeenCalled();
    await vi.advanceTimersByTimeAsync(20000);
    expect(FakeSocket.live).toHaveLength(1);
  });
});

describe("being dropped", () => {
  it("reports a 401 and tells the shell", async () => {
    fetchMock.mockResolvedValue(json({ error: "not paired" }, 401));
    const revoked = vi.fn();
    await expect(remoteApi(follow, revoked).getState()).rejects.toThrow("The host revoked this");
    expect(revoked).toHaveBeenCalled();
  });

  it("by default forgets the host and leaves a note for the reload", () => {
    const store = new Map<string, string>([["da.companion.follow", "{}"]]);
    vi.stubGlobal("localStorage", {
      getItem: (k: string) => store.get(k) ?? null,
      setItem: (k: string, v: string) => void store.set(k, v),
      removeItem: (k: string) => void store.delete(k),
    });
    const reload = vi.fn();
    vi.stubGlobal("location", { reload });
    fetchMock.mockResolvedValue(json({ error: "not paired" }, 401));
    return remoteApi(follow)
      .getState()
      .catch(() => {
        expect(store.has("da.companion.follow")).toBe(false);
        expect(store.get(REVOKED_KEY)).toBe("1");
        expect(reload).toHaveBeenCalled();
      });
  });
});

describe("what the host keeps", () => {
  it("refuses every host-only call by name", async () => {
    const api = remoteApi(follow, () => undefined);
    for (const call of [
      () => api.setApiKey("sk-x"),
      () => api.setChatBudget(9),
      () => api.yahooLeagues(),
      () => api.recordManualPick("1"),
      () => api.exportState(),
      () => api.askClaude({ screen: "draft", model: "Opus 5", effort: "High", messages: [] }),
      () => api.companionEnable(),
    ]) {
      await expect(call()).rejects.toThrow("That's controlled by the host (Justin's Mac)");
    }
  });

  it("restores the host's own league as a read, and refuses to switch it", async () => {
    // The shell re-adds its active league on every boot; on a follower that
    // must come back as the host's board, not as a refusal.
    fetchMock.mockResolvedValue(json(draftView));
    const api = remoteApi(follow, () => undefined);
    await expect(api.addLeague("L1")).resolves.toMatchObject({ league: { league_id: "L1" } });
    expect(fetchMock).toHaveBeenLastCalledWith(
      "http://192.168.1.5:7878/api/state",
      expect.anything(),
    );
    await expect(api.addLeague("L2")).rejects.toThrow(/Justin's Mac is on a different league/);
  });

  it("still asks and sends on the shared thread", async () => {
    fetchMock.mockResolvedValue(
      json({ league_id: "L1", screen: "draft", busy: false, entries: [] }),
    );
    const api = remoteApi(follow, () => undefined);
    await expect(api.sharedChatGet("draft")).resolves.toMatchObject({ screen: "draft" });
    fetchMock.mockResolvedValue(json({ entry_id: "e1" }, 202));
    await api.sharedChatSend("draft", "who should I take?");
    expect(fetchMock).toHaveBeenLastCalledWith(
      "http://192.168.1.5:7878/api/chat",
      expect.objectContaining({ method: "POST" }),
    );
  });

  it("says who is answering when the thread is busy", async () => {
    fetchMock.mockResolvedValue(json({ error: "busy" }, 409));
    await expect(remoteApi(follow, () => undefined).sharedChatSend("draft", "hi")).rejects.toThrow(
      /Someone else is asking/,
    );
  });
});
