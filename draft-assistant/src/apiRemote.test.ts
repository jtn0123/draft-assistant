// The follower's backend, driven against a fake host: a stubbed `fetch` for
// the reads and a fake WebSocket for the live half.

import { afterEach, beforeEach, describe, expect, it, vi } from "vitest";
import { remoteApi, remoteFetcher, REVOKED_KEY } from "./apiRemote";
import { getFollowStatus, resetFollowStatus } from "./followStatus";
import type { Api } from "./api";
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

const seasonView = { schema_version: "1.3", week: 3 } as unknown as Record<string, unknown>;

/** A stand-in for the browser's socket that a test can push frames into. */
class FakeSocket {
  static live: FakeSocket[] = [];
  onopen: (() => void) | null = null;
  onmessage: ((event: { data: string }) => void) | null = null;
  onclose: ((event?: { code?: number }) => void) | null = null;
  onerror: (() => void) | null = null;
  sent: string[] = [];
  closed = false;
  constructor(readonly url: string) {
    FakeSocket.live.push(this);
  }
  send(text: string): void {
    this.sent.push(text);
  }
  close(code?: number): void {
    this.closed = true;
    this.onclose?.({ code });
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
  resetFollowStatus();
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

  it("by default forgets the host and leaves a note, without pulling the page out", async () => {
    const store = new Map<string, string>([["da.companion.follow", "{}"]]);
    vi.stubGlobal("localStorage", {
      getItem: (k: string) => store.get(k) ?? null,
      setItem: (k: string, v: string) => void store.set(k, v),
      removeItem: (k: string) => void store.delete(k),
    });
    const reload = vi.fn();
    vi.stubGlobal("location", { reload });
    fetchMock.mockResolvedValue(json({ error: "not paired" }, 401));
    await expect(remoteApi(follow).getState()).rejects.toThrow("The host revoked this");

    expect(store.has("da.companion.follow")).toBe(false);
    expect(store.get(REVOKED_KEY)).toBe("1");
    // The header says what happened and offers the way back, so the window
    // is left standing rather than reloaded out from under the user.
    expect(reload).not.toHaveBeenCalled();
    expect(getFollowStatus()).toBe("revoked");
  });
});

describe("the connection state the header reads", () => {
  it("starts connected and says so when nothing is wrong", async () => {
    await remoteApi(follow, () => undefined).onDraftUpdated(() => undefined);
    newest().onopen?.();
    expect(getFollowStatus()).toBe("connected");
  });

  it("says it is reconnecting while the socket is away, and connected once back", async () => {
    vi.useFakeTimers();
    await remoteApi(follow, () => undefined).onDraftUpdated(() => undefined);
    newest().onopen?.();
    newest().close();
    expect(getFollowStatus()).toBe("reconnecting");

    await vi.advanceTimersByTimeAsync(1000);
    newest().onopen?.();
    expect(getFollowStatus()).toBe("connected");
  });

  it("reads a 4401 close as being dropped, and stops trying", async () => {
    vi.useFakeTimers();
    const revoked = vi.fn();
    await remoteApi(follow, revoked).onDraftUpdated(() => undefined);
    newest().close(4401);

    expect(revoked).toHaveBeenCalled();
    expect(getFollowStatus()).toBe("revoked");
    await vi.advanceTimersByTimeAsync(20000);
    expect(FakeSocket.live).toHaveLength(1);
  });

  it("never walks a revoked device back to reconnecting", async () => {
    const api = remoteApi(follow, () => undefined);
    await api.onDraftUpdated(() => undefined);
    newest().close(4401);
    // A second socket cannot exist after 4401, but the state must not be
    // reopened by anything that closes late either.
    newest().onclose?.({ code: 1006 });
    expect(getFollowStatus()).toBe("revoked");
  });

  it("marks the follower revoked when any call is answered with a 401", async () => {
    fetchMock.mockResolvedValue(json({ error: "not paired" }, 401));
    await expect(remoteApi(follow, () => undefined).getState()).rejects.toThrow(
      "The host revoked this",
    );
    expect(getFollowStatus()).toBe("revoked");
  });
});

describe("coming back from a blip", () => {
  it("re-reads the board and the season every time the socket opens", async () => {
    // Nothing else re-read state after a reconnect: whatever the host did
    // while the socket was away simply never arrived.
    vi.useFakeTimers();
    fetchMock.mockImplementation((url: string) =>
      Promise.resolve(json(url.endsWith("/api/season") ? seasonView : draftView)),
    );
    const api = remoteApi(follow, () => undefined);
    const boards: DraftView[] = [];
    const seasons: unknown[] = [];
    await api.onDraftUpdated((view) => boards.push(view));
    await api.onSeasonUpdated((view) => seasons.push(view));

    newest().onopen?.();
    await vi.advanceTimersByTimeAsync(0);
    expect(boards).toHaveLength(1);
    expect(seasons).toHaveLength(1);

    newest().close();
    await vi.advanceTimersByTimeAsync(1000);
    expect(FakeSocket.live).toHaveLength(2);
    newest().onopen?.();
    await vi.advanceTimersByTimeAsync(0);
    expect(boards).toHaveLength(2);
  });

  it("asks for nothing no screen is listening for", async () => {
    fetchMock.mockResolvedValue(json(draftView));
    const api = remoteApi(follow, () => undefined);
    await api.onPollHealth(() => undefined);
    newest().onopen?.();
    await Promise.resolve();
    expect(fetchMock).not.toHaveBeenCalled();
  });

  it("stays up when the host answers the re-read with an error", async () => {
    fetchMock.mockRejectedValue(new Error("connection refused"));
    const api = remoteApi(follow, () => undefined);
    await api.onDraftUpdated(() => undefined);
    newest().onopen?.();
    await vi.waitFor(() => expect(fetchMock).toHaveBeenCalled());
    expect(getFollowStatus()).toBe("connected");
  });
});

/** Every call a follower refuses outright, with arguments good enough to
 *  reach the refusal. Seven of these were covered and thirteen were not, so a
 *  new host-only method could be added and quietly answer nothing at all. Add
 *  a row here whenever `remoteApi` gains a `refused`. */
const hostOnlyCalls: Array<[string, (api: Api) => Promise<unknown>]> = [
  ["removeLeague", (api) => api.removeLeague("L1")],
  ["setMyUsername", (api) => api.setMyUsername("justin")],
  ["setApiKey", (api) => api.setApiKey("sk-x")],
  ["setChatBudget", (api) => api.setChatBudget(9)],
  ["setChatProvider", (api) => api.setChatProvider("api")],
  ["yahooSaveCredentials", (api) => api.yahooSaveCredentials("id", "secret")],
  ["yahooBeginConnect", (api) => api.yahooBeginConnect()],
  ["yahooFinishConnect", (api) => api.yahooFinishConnect("code", "state")],
  ["yahooDisconnect", (api) => api.yahooDisconnect(false)],
  ["yahooLeagues", (api) => api.yahooLeagues()],
  ["importSecondOpinion", (api) => api.importSecondOpinion()],
  ["recordManualPick", (api) => api.recordManualPick("1")],
  ["undoManualPick", (api) => api.undoManualPick()],
  ["exportState", (api) => api.exportState()],
  [
    "askClaude",
    (api) => api.askClaude({ screen: "draft", model: "Opus 5", effort: "High", messages: [] }),
  ],
  ["companionStatus", (api) => api.companionStatus()],
  ["companionEnable", (api) => api.companionEnable()],
  ["companionDisable", (api) => api.companionDisable()],
  ["companionRevoke", (api) => api.companionRevoke()],
  ["setDeviceName", (api) => api.setDeviceName("Justin's Mac")],
];

describe("what the host keeps", () => {
  it.each(hostOnlyCalls)("refuses %s by naming the host", async (_name, call) => {
    await expect(call(remoteApi(follow, () => undefined))).rejects.toThrow(
      "That's controlled by the host (Justin's Mac)",
    );
    // A refusal is a decision, not a request: nothing may go out over the wire.
    expect(fetchMock).not.toHaveBeenCalled();
  });

  it("covers every host-only method the follower refuses", () => {
    // The count is the guard: adding a `refused` without a row here fails.
    expect(hostOnlyCalls).toHaveLength(20);
    expect(new Set(hostOnlyCalls.map(([name]) => name)).size).toBe(hostOnlyCalls.length);
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

describe("a socket that is open but not listening", () => {
  it("drops and reconnects once two pings go unanswered", async () => {
    // The failure this prevents: a laptop that slept, or a host that was force
    // quit, leaves a socket the browser goes on calling open. Nothing was read
    // back off the ping, so the follower sat on a board that had stopped
    // moving and the header still said it was connected.
    vi.useFakeTimers();
    await remoteApi(follow, () => undefined).onDraftUpdated(() => undefined);
    const first = newest();
    first.onopen?.();

    await vi.advanceTimersByTimeAsync(25000);
    await vi.advanceTimersByTimeAsync(25000);
    expect(first.closed).toBe(false);
    expect(getFollowStatus()).toBe("connected");

    // The third tick finds two pings still unanswered.
    await vi.advanceTimersByTimeAsync(25000);
    expect(first.closed).toBe(true);
    expect(getFollowStatus()).toBe("reconnecting");

    await vi.advanceTimersByTimeAsync(1000);
    expect(FakeSocket.live).toHaveLength(2);
  });

  it("stays up for as long as the host keeps answering", async () => {
    vi.useFakeTimers();
    await remoteApi(follow, () => undefined).onDraftUpdated(() => undefined);
    const socket = newest();
    socket.onopen?.();

    for (let i = 0; i < 6; i += 1) {
      await vi.advanceTimersByTimeAsync(25000);
      socket.push("pong");
    }

    expect(socket.closed).toBe(false);
    expect(FakeSocket.live).toHaveLength(1);
    expect(getFollowStatus()).toBe("connected");
  });

  it("re-reads the board when the window comes back or the network does", async () => {
    // Counted through this window's own subscriber rather than the shared
    // fetch mock: every other case in this file leaves a live follower behind,
    // and they all answer these two events too.
    fetchMock.mockResolvedValue(json(draftView));
    const api = remoteApi(follow, () => undefined);
    const boards: DraftView[] = [];
    await api.onDraftUpdated((view) => boards.push(view));
    newest().onopen?.();
    await vi.waitFor(() => expect(boards).toHaveLength(1));

    document.dispatchEvent(new Event("visibilitychange"));
    await vi.waitFor(() => expect(boards).toHaveLength(2));

    window.dispatchEvent(new Event("online"));
    await vi.waitFor(() => expect(boards).toHaveLength(3));
  });
});
