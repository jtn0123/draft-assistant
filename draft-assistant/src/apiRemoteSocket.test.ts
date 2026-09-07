// The follower's one shared socket: what it delivers, what it does when the
// host drops it or forgets this device, and what it takes off the frames the
// host opens and answers with. The HTTP half is in `apiRemote.test.ts`.

import { describe, expect, it, vi } from "vitest";
import { remoteApi, REVOKED_KEY } from "./apiRemote";
import { getFollowStatus } from "./followStatus";
import { getHostSync } from "./hostSync";
import {
  draftView,
  fetchMock,
  FakeSocket,
  follow,
  installFakeHost,
  json,
  newest,
  seasonView,
} from "./test/remoteHost";
import type { DraftView, FollowRecord } from "./types";

installFakeHost();

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

describe("what the host says about itself", () => {
  /** A follower with one live socket, ready to be pushed frames. */
  async function connected(record: FollowRecord = { ...follow }) {
    const api = remoteApi(record, () => undefined);
    await api.onDraftUpdated(() => undefined);
    newest().onopen?.();
    return { api, record };
  }

  it("takes the host's live sync off the opening frame and every heartbeat after", async () => {
    // The failure this prevents: a follower's `startPolling` is a no-op that
    // resolves, so the shell set its flag true whatever the host was doing
    // and the header showed a green Live pill over a board whose host had
    // switched live sync off. Nothing carried the host's own flag across.
    await connected();
    expect(getHostSync().polling).toBe(null);
    newest().push("hello", { server_now_ms: 1, host_name: "Justin's Mac", polling: false });
    expect(getHostSync()).toEqual({ polling: false, hostName: "Justin's Mac" });
    newest().push("pong", { server_now_ms: 2, host_name: "Justin's Mac", polling: true });
    expect(getHostSync().polling).toBe(true);
  });

  it("carries a renamed host into the record every message is built from", async () => {
    // The failure this prevents: the name was taken once, from the answer to
    // `POST /api/pair`, so changing "Your name in shared chat" on the host
    // left every follower calling it by the name it joined with.
    const { api, record } = await connected();
    newest().push("hello", { server_now_ms: 1, host_name: "The Big Board", polling: true });
    expect(record.host_name).toBe("The Big Board");
    expect(getHostSync().hostName).toBe("The Big Board");
    await expect(api.setApiKey("k")).rejects.toThrow("The Big Board");
    // And written back, so a reload does not go to the old name either.
    const saved = JSON.parse(localStorage.getItem("da.companion.follow") ?? "{}") as FollowRecord;
    expect(saved.host_name).toBe("The Big Board");
  });

  it("leaves what it knows alone when a frame says nothing about it", async () => {
    await connected();
    newest().push("hello", { server_now_ms: 1, host_name: "Justin's Mac", polling: true });
    newest().push("pong", { server_now_ms: 2 });
    expect(getHostSync()).toEqual({ polling: true, hostName: "Justin's Mac" });
  });
});
