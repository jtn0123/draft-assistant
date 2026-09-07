// Where a follower's own failures go: the local backend's log inside Tauri,
// the console and a ring buffer in a browser tab. The failure this guards is
// the one before it existed, a follower whose `logFrontendError` did nothing
// and whose crashes left no line anywhere.

import { afterEach, beforeEach, describe, expect, it, vi } from "vitest";

const invoke = vi.hoisted(() => vi.fn());
vi.mock("@tauri-apps/api/core", () => ({ invoke }));

import { remoteApi } from "./apiRemote";
import { followerLog, followerLogDiagnostics, resetFollowerLog } from "./apiRemoteLog";
import { resetFollowStatus } from "./followStatus";
import type { FollowRecord } from "./types";

const follow: FollowRecord = {
  url: "http://192.168.1.5:7878",
  token: "tok-1",
  host_name: "Justin's Mac",
};

const tauriWindow = window as unknown as { __TAURI_INTERNALS__?: unknown };

beforeEach(() => {
  resetFollowStatus();
  resetFollowerLog();
  invoke.mockReset();
  vi.stubGlobal("WebSocket", class {});
  vi.stubGlobal(
    "fetch",
    vi.fn(() => Promise.resolve({ ok: false, status: 404 })),
  );
});

afterEach(() => {
  delete tauriWindow.__TAURI_INTERNALS__;
  vi.unstubAllGlobals();
  vi.restoreAllMocks();
});

describe("inside the desktop app", () => {
  beforeEach(() => {
    tauriWindow.__TAURI_INTERNALS__ = {};
  });

  it("writes the follower's own log through its own backend, never the host's", async () => {
    invoke.mockResolvedValue(undefined);
    const fetchMock = vi.mocked(fetch);
    await remoteApi(follow, () => undefined).logFrontendError(
      "TypeError: x is undefined",
      "render",
      "at Board (app.js:1:1)",
    );
    expect(invoke).toHaveBeenCalledWith("log_frontend_error", {
      message: "TypeError: x is undefined",
      source: "render",
      stack: "at Board (app.js:1:1)",
    });
    expect(fetchMock).not.toHaveBeenCalled();
  });

  it("names the local log file in the diagnostics, with the host still in the platform line", async () => {
    invoke.mockResolvedValue({
      app_version: "1.4.0",
      log_path: "/Users/rob/Library/Logs/draft-assistant/app.log",
      log_level: "debug",
      log_tail: ["2026-09-07T10:00:00Z ERROR frontend: boom where=render"],
    });
    const report = await remoteApi(follow, () => undefined).diagnostics();
    expect(report).toMatchObject({
      app_version: "1.4.0",
      platform: "following Justin's Mac",
      log_path: "/Users/rob/Library/Logs/draft-assistant/app.log",
      log_level: "debug",
      log_tail: ["2026-09-07T10:00:00Z ERROR frontend: boom where=render"],
    });
    expect(invoke).toHaveBeenCalledWith("diagnostics");
  });

  it("falls back to what this window kept when the local backend does not answer", async () => {
    invoke.mockRejectedValue(new Error("no backend"));
    const report = await remoteApi(follow, () => undefined).diagnostics();
    expect(report.log_path).toBeNull();
  });
});

describe("in a browser tab", () => {
  it("writes the line to the console and keeps it for the diagnostics", async () => {
    const consoleError = vi.spyOn(console, "error").mockImplementation(() => undefined);
    await followerLog("TypeError: x is undefined", "render", "at Board (app.js:1:1)");
    expect(invoke).not.toHaveBeenCalled();
    expect(consoleError).toHaveBeenCalledTimes(1);
    const [line] = consoleError.mock.calls[0] as [string];
    expect(line).toMatch(/ERROR frontend: TypeError: x is undefined where=render\nat Board/);
    const report = await followerLogDiagnostics();
    expect(report.log_path).toBeNull();
    expect(report.log_tail).toEqual([line]);
  });

  it("keeps the last fifty lines and no more", async () => {
    vi.spyOn(console, "error").mockImplementation(() => undefined);
    for (let n = 0; n < 60; n += 1) await followerLog(`failure ${n}`, "poll");
    const { log_tail } = await followerLogDiagnostics();
    expect(log_tail).toHaveLength(50);
    expect(log_tail[0]).toContain("failure 10");
    expect(log_tail[49]).toContain("failure 59");
  });
});
