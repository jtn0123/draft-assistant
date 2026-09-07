// The fake host every `apiRemote` test runs against: one stubbed `fetch` for
// the reads, one fake WebSocket for the live half, and the reset that puts
// both back between tests.
//
// Shared by `apiRemote.test.ts` (the reads and the calls a follower refuses)
// and `apiRemoteSocket.test.ts` (the socket: dropping, reconnecting, and what
// the host says about itself), which were one file until it crossed the
// repository's size cap.

import { afterEach, beforeEach, vi } from "vitest";
import { resetFollowStatus } from "../followStatus";
import { resetHostSync } from "../hostSync";
import type { DraftView, FollowRecord } from "../types";

export const follow: FollowRecord = {
  url: "http://192.168.1.5:7878",
  token: "tok-1",
  host_name: "Justin's Mac",
};

export const draftView = {
  schema_version: "1.5",
  league: { league_id: "L1", name: "Test", season: "2026", platform: "sleeper" },
} as unknown as DraftView;

export const seasonView = { schema_version: "1.4", week: 3 } as unknown as Record<string, unknown>;

/** A stand-in for the browser's socket that a test can push frames into. */
export class FakeSocket {
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

export const newest = (): FakeSocket => {
  const socket = FakeSocket.live[FakeSocket.live.length - 1];
  if (socket === undefined) throw new Error("no socket was opened");
  return socket;
};

export const fetchMock = vi.fn();

/** A JSON answer with a status. */
export function json(body: unknown, status = 200): Response {
  return {
    ok: status >= 200 && status < 300,
    status,
    headers: new Headers({ "content-type": "application/json" }),
    json: () => Promise.resolve(body),
  } as unknown as Response;
}

/** Give this file's tests the fake host, and take it away again. Called at
 *  the top of each test file rather than run on import, so a file that does
 *  not want the globals stubbed does not get them. */
export function installFakeHost(): void {
  beforeEach(() => {
    resetFollowStatus();
    resetHostSync();
    localStorage.clear();
    FakeSocket.live = [];
    fetchMock.mockReset();
    vi.stubGlobal("fetch", fetchMock);
    vi.stubGlobal("WebSocket", FakeSocket);
  });

  afterEach(() => {
    vi.unstubAllGlobals();
    vi.useRealTimers();
  });
}
