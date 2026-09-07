// The follower's writes and images against a host that takes the connection
// and never answers. The reads have had a deadline for a while; these were
// bare `fetch` calls, and a Send that hung for ever with no error was the
// result.

import { afterEach, beforeEach, describe, expect, it, vi } from "vitest";
import { HOST_TIMEOUT, HOST_TIMEOUT_MS, remoteApi } from "./apiRemote";
import { resetFollowStatus } from "./followStatus";
import type { FollowRecord } from "./types";

const follow: FollowRecord = {
  url: "http://192.168.1.5:7878",
  token: "tok-1",
  host_name: "Justin's Mac",
};

const fetchMock = vi.fn();

/** A fetch that never answers on its own, only when its signal fires. */
function silentHost() {
  fetchMock.mockImplementation(
    (_url: string, init: RequestInit) =>
      new Promise((_resolve, reject) => {
        init.signal?.addEventListener("abort", () => reject(new Error("aborted")));
      }),
  );
}

beforeEach(() => {
  resetFollowStatus();
  fetchMock.mockReset();
  vi.stubGlobal("fetch", fetchMock);
  vi.stubGlobal("WebSocket", class {});
  vi.useFakeTimers();
});

afterEach(() => {
  vi.unstubAllGlobals();
  vi.useRealTimers();
});

describe("a host that never answers a write", () => {
  it("gives up on a question after the deadline, by name", async () => {
    silentHost();
    const sent = remoteApi(follow, () => undefined).sharedChatSend("draft", "who?");
    const failed = expect(sent).rejects.toMatchObject({
      name: HOST_TIMEOUT,
      message: "Justin's Mac did not answer within 10 seconds",
    });
    await vi.advanceTimersByTimeAsync(HOST_TIMEOUT_MS);
    await failed;
  });

  it("gives up on emptying the thread after the deadline", async () => {
    silentHost();
    const reset = remoteApi(follow, () => undefined).sharedChatReset("draft");
    const failed = expect(reset).rejects.toThrow(/did not answer within/);
    await vi.advanceTimersByTimeAsync(HOST_TIMEOUT_MS);
    await failed;
  });

  it("sends the token and the body with the deadline, not instead of them", async () => {
    fetchMock.mockResolvedValue({ ok: true, status: 202 });
    await remoteApi(follow, () => undefined).sharedChatSend("draft", "who?");
    expect(fetchMock).toHaveBeenCalledWith(
      "http://192.168.1.5:7878/api/chat",
      expect.objectContaining({
        method: "POST",
        headers: { authorization: "Bearer tok-1", "content-type": "application/json" },
        body: JSON.stringify({ screen: "draft", text: "who?" }),
        signal: expect.any(AbortSignal) as AbortSignal,
      }),
    );
  });
});

describe("a host that never answers for a picture", () => {
  it("shows the blank after the deadline rather than pinning the request for ever", async () => {
    silentHost();
    const headshot = remoteApi(follow, () => undefined).headshot("4046");
    await vi.advanceTimersByTimeAsync(HOST_TIMEOUT_MS);
    await expect(headshot).resolves.toBeNull();
  });
});
