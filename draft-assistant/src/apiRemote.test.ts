// The follower's backend over HTTP: the reads, the deadline on a host that
// goes quiet, and every call it refuses because the host owns it. The socket
// half is in `apiRemoteSocket.test.ts`.

import { describe, expect, it, vi } from "vitest";
import { HOST_TIMEOUT_MS, remoteApi, remoteFetcher } from "./apiRemote";
import { draftView, fetchMock, follow, installFakeHost, json } from "./test/remoteHost";
import type { Api } from "./api";

installFakeHost();

describe("reads", () => {
  it("gets the state from the host with the pairing token", async () => {
    fetchMock.mockResolvedValue(json(draftView));
    const view = await remoteApi(follow, () => undefined).getState();
    expect(view.league.league_id).toBe("L1");
    expect(fetchMock).toHaveBeenCalledWith(
      "http://192.168.1.5:7878/api/state",
      expect.objectContaining({ headers: { authorization: "Bearer tok-1" } }),
    );
  });

  it("reads a 404 as nothing there, and says so by name", async () => {
    fetchMock.mockResolvedValue(json({ error: "no league loaded" }, 404));
    await expect(remoteFetcher(follow, () => undefined)("/api/season")).resolves.toBeNull();
    await expect(remoteApi(follow, () => undefined).getSeason()).rejects.toThrow(
      /Justin's Mac hasn't opened the Season screen yet/,
    );
    // Copy the user reads: two sentences, not an em-dash.
    await expect(remoteApi(follow, () => undefined).getSeason()).rejects.toThrow(/^[^—]*$/);
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

describe("a host that accepts the connection and then says nothing", () => {
  /** A fetch that never answers on its own, only when its signal fires. */
  function silentHost() {
    fetchMock.mockImplementation(
      (_url: string, init: RequestInit) =>
        new Promise((_resolve, reject) => {
          init.signal?.addEventListener("abort", () => reject(new Error("aborted")));
        }),
    );
  }

  it("gives up after ten seconds instead of leaving the launch screen with no controls", async () => {
    vi.useFakeTimers();
    silentHost();
    const read = remoteFetcher(follow, () => undefined)("/api/state");
    const failed = expect(read).rejects.toThrow("Justin's Mac did not answer within 10 seconds");
    await vi.advanceTimersByTimeAsync(HOST_TIMEOUT_MS);
    await failed;
  });

  it("reaches the shell as the launch error, not as a hang", async () => {
    vi.useFakeTimers();
    silentHost();
    const restore = remoteApi(follow, () => undefined).addLeague("L1");
    const failed = expect(restore).rejects.toThrow(/did not answer/);
    await vi.advanceTimersByTimeAsync(HOST_TIMEOUT_MS);
    await failed;
  });

  it("lets an answer that arrives in time through untouched", async () => {
    vi.useFakeTimers();
    fetchMock.mockResolvedValue(json(draftView));
    await expect(remoteFetcher(follow, () => undefined)("/api/state")).resolves.toEqual(draftView);
    // The timer is cleared with the answer: winding the clock on afterwards
    // must not abort anything or leave a rejection nobody is waiting on.
    await vi.advanceTimersByTimeAsync(HOST_TIMEOUT_MS);
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
    // Copy the user reads: two sentences, not an em-dash.
    await expect(remoteApi(follow, () => undefined).sharedChatSend("draft", "hi")).rejects.toThrow(
      /^[^—]*$/,
    );
  });
});
