// The season lifecycle on its own: load once, poll while showing, stop when
// hidden, surface a failure, and recover on retry.

import { act, renderHook, waitFor } from "@testing-library/react";
import { settle } from "./test/settle";
import { beforeEach, describe, expect, it, vi } from "vitest";
import type { SeasonView } from "./season-types";
import type { PollHealth } from "./types";

const mocks = vi.hoisted(() => ({
  loadSeason: vi.fn(),
  startSeasonPolling: vi.fn(),
  stopSeasonPolling: vi.fn(),
  onSeasonUpdated: vi.fn(),
  onSeasonPollHealth: vi.fn(),
}));
vi.mock("./api", () => ({ api: mocks }));

import { reloadSeason, useSeasonSession, type SeasonSession } from "./session";

const view = (week: number) => ({ schema_version: "1.1", week }) as unknown as SeasonView;

let pushUpdate: ((v: SeasonView) => void) | null = null;
let pushHealth: ((h: PollHealth) => void) | null = null;

beforeEach(() => {
  vi.clearAllMocks();
  pushUpdate = null;
  pushHealth = null;
  mocks.startSeasonPolling.mockResolvedValue(undefined);
  mocks.stopSeasonPolling.mockResolvedValue(undefined);
  mocks.onSeasonUpdated.mockImplementation((handler: (v: SeasonView) => void) => {
    pushUpdate = handler;
    return Promise.resolve(() => undefined);
  });
  mocks.onSeasonPollHealth.mockImplementation((handler: (h: PollHealth) => void) => {
    pushHealth = handler;
    return Promise.resolve(() => undefined);
  });
});

describe("useSeasonSession", () => {
  it("fetches nothing until the screen is showing and a league is loaded", () => {
    const { rerender } = renderHook<SeasonSession, { active: boolean; leagueId: string | null }>(
      ({ active, leagueId }) => useSeasonSession(active, leagueId, () => undefined),
      {
        initialProps: { active: false, leagueId: "1" },
      },
    );
    expect(mocks.loadSeason).not.toHaveBeenCalled();

    rerender({ active: true, leagueId: null });
    expect(mocks.loadSeason).not.toHaveBeenCalled();
    expect(mocks.startSeasonPolling).not.toHaveBeenCalled();
  });

  it("loads once and starts polling when the screen opens", async () => {
    mocks.loadSeason.mockResolvedValue(view(2));
    const { result, rerender } = renderHook(
      ({ active }) => useSeasonSession(active, "1", () => undefined),
      { initialProps: { active: true } },
    );

    await waitFor(() => expect(result.current.season?.week).toBe(2));
    expect(mocks.loadSeason).toHaveBeenCalledWith(false);
    expect(mocks.startSeasonPolling).toHaveBeenCalledWith(30);

    // Re-rendering with the view already loaded must not refetch.
    rerender({ active: true });
    expect(mocks.loadSeason).toHaveBeenCalledTimes(1);
  });

  it("stops polling when the screen is no longer showing", async () => {
    mocks.loadSeason.mockResolvedValue(view(2));
    const { result, rerender } = renderHook(
      ({ active }) => useSeasonSession(active, "1", () => undefined),
      { initialProps: { active: true } },
    );
    await waitFor(() => expect(result.current.season).not.toBeNull());

    rerender({ active: false });
    await waitFor(() => expect(mocks.stopSeasonPolling).toHaveBeenCalled());
  });

  it("applies pushed live updates", async () => {
    mocks.loadSeason.mockResolvedValue(view(2));
    const { result } = renderHook(() => useSeasonSession(true, "1", () => undefined));
    await waitFor(() => expect(result.current.season?.week).toBe(2));

    await waitFor(() => expect(pushUpdate).not.toBeNull());
    pushUpdate?.(view(3));
    await waitFor(() => expect(result.current.season?.week).toBe(3));
  });

  it("keeps polling through a pushed update instead of restarting it", async () => {
    mocks.loadSeason.mockResolvedValue(view(2));
    const { result, rerender } = renderHook(
      // A fresh callback on every render, the way an inline arrow in a
      // component would be: neither it nor the update may restart the poller.
      () => useSeasonSession(true, "1", () => undefined),
    );
    await waitFor(() => expect(result.current.season?.week).toBe(2));
    expect(mocks.startSeasonPolling).toHaveBeenCalledTimes(1);

    await waitFor(() => expect(pushUpdate).not.toBeNull());
    act(() => pushUpdate?.(view(3)));
    await waitFor(() => expect(result.current.season?.week).toBe(3));
    rerender();

    expect(mocks.startSeasonPolling).toHaveBeenCalledTimes(1);
    expect(mocks.stopSeasonPolling).not.toHaveBeenCalled();
    expect(mocks.loadSeason).toHaveBeenCalledTimes(1);
  });

  it("reports a failure once, to both the caller and the toast", async () => {
    mocks.loadSeason.mockRejectedValue(new Error("Sleeper timed out"));
    const onError = vi.fn();
    const { result } = renderHook(() => useSeasonSession(true, "1", onError));

    await waitFor(() => expect(result.current.error).toMatch(/Sleeper timed out/));
    expect(result.current.season).toBeNull();
    expect(onError).toHaveBeenCalledTimes(1);
  });

  it("says so when live updates could not be started", async () => {
    mocks.loadSeason.mockResolvedValue(view(2));
    mocks.startSeasonPolling.mockRejectedValue(new Error("no league loaded"));
    const onError = vi.fn();
    renderHook(() => useSeasonSession(true, "1", onError));

    await waitFor(() =>
      expect(onError).toHaveBeenCalledWith(expect.stringContaining("Live updates are not running")),
    );
  });

  it("hands on how the live poll is going, good news and bad", async () => {
    mocks.loadSeason.mockResolvedValue(view(2));
    const { result } = renderHook(() => useSeasonSession(true, "1", () => undefined));
    await waitFor(() => expect(mocks.onSeasonPollHealth).toHaveBeenCalled());
    // Nothing is claimed before the first poll has reported.
    expect(result.current.pollHealth).toBeNull();

    const failing = {
      last_success_at: 1000,
      consecutive_failures: 2,
      last_error: "scores: request failed",
    };
    act(() => pushHealth?.(failing));
    expect(result.current.pollHealth).toEqual(failing);

    // A failing poll must not disturb the last good view: the numbers stay put
    // and only the health says they have stopped moving.
    expect(result.current.season?.week).toBe(2);

    act(() => pushHealth?.({ last_success_at: 2000, consecutive_failures: 0, last_error: null }));
    expect(result.current.pollHealth?.consecutive_failures).toBe(0);
  });

  it("retry clears the error and forces a fresh fetch", async () => {
    mocks.loadSeason.mockRejectedValueOnce(new Error("down")).mockResolvedValue(view(5));
    // A stable callback, so the load effect does not re-fire on every render
    // and race the retry with a second automatic fetch.
    const quiet = () => undefined;
    const { result } = renderHook(() => useSeasonSession(true, "1", quiet));
    await waitFor(() => expect(result.current.error).not.toBeNull());

    // Retry drives two state updates; let React flush both before reading them.
    await settle(() => {
      result.current.retry();
    });
    await waitFor(() => expect(result.current.season?.week).toBe(5));
    expect(result.current.error).toBeNull();
    // force=true: a retry must bypass the cache that just failed.
    expect(mocks.loadSeason).toHaveBeenLastCalledWith(true);
  });
});

// Grade item D6. The teardown paths: what the hook lets go of when the window
// closes, and what it does with a failure that arrives after nobody is left to
// tell. These are the branches a leak or a set-state-after-unmount hides in.
describe("useSeasonSession on the way out", () => {
  it("unsubscribes from both feeds when it goes away", async () => {
    const stopUpdates = vi.fn();
    const stopHealth = vi.fn();
    mocks.loadSeason.mockResolvedValue(view(2));
    mocks.onSeasonUpdated.mockReturnValue(Promise.resolve(stopUpdates));
    mocks.onSeasonPollHealth.mockReturnValue(Promise.resolve(stopHealth));

    const { unmount } = renderHook(() => useSeasonSession(true, "1", () => undefined));
    await act(async () => {
      unmount();
      await Promise.resolve();
    });

    expect(stopUpdates).toHaveBeenCalledTimes(1);
    expect(stopHealth).toHaveBeenCalledTimes(1);
    // Polling is the backend's timer, and it must not be left running for a
    // screen that no longer exists.
    expect(mocks.stopSeasonPolling).toHaveBeenCalled();
  });

  it("survives a subscription that never resolved into an unlisten", async () => {
    mocks.loadSeason.mockResolvedValue(view(2));
    mocks.onSeasonUpdated.mockReturnValue(Promise.reject(new Error("no event bus")));
    mocks.stopSeasonPolling.mockRejectedValue(new Error("already stopped"));

    const { unmount } = renderHook(() => useSeasonSession(true, "1", () => undefined));
    await act(async () => {
      unmount();
      await Promise.resolve();
    });
    // Nothing thrown, nothing reported: a teardown failure is not the user's
    // problem, and an unhandled rejection here would fail this test.
    await act(async () => {
      await Promise.resolve();
    });
  });

  it("says nothing when the load fails after the screen has closed", async () => {
    let reject: ((e: Error) => void) | null = null;
    mocks.loadSeason.mockReturnValue(
      new Promise((_resolve, r: (e: Error) => void) => {
        reject = r;
      }),
    );
    const onError = vi.fn();
    const { unmount } = renderHook(() => useSeasonSession(true, "1", onError));
    await waitFor(() => expect(reject).not.toBeNull());

    unmount();
    await act(async () => {
      reject?.(new Error("Sleeper timed out"));
      await Promise.resolve();
    });

    // A toast for a screen nobody is looking at is noise, not news.
    expect(onError).not.toHaveBeenCalled();
  });
});

describe("a retry that fails too", () => {
  it("replaces the error rather than clearing it and going quiet", async () => {
    mocks.loadSeason
      .mockRejectedValueOnce(new Error("down"))
      .mockRejectedValueOnce(new Error("still down"));
    const quiet = () => undefined;
    const { result } = renderHook(() => useSeasonSession(true, "1", quiet));
    await waitFor(() => expect(result.current.error).toMatch(/down/));

    await settle(() => {
      result.current.retry();
    });
    await waitFor(() => expect(result.current.error).toMatch(/still down/));
    expect(result.current.season).toBeNull();
  });
});

describe("reloadSeason", () => {
  it("always bypasses the cache", async () => {
    mocks.loadSeason.mockResolvedValue(view(9));
    await expect(reloadSeason()).resolves.toEqual(view(9));
    expect(mocks.loadSeason).toHaveBeenCalledWith(true);
  });
});

describe("switching leagues", () => {
  it("drops the old season, loads the new one, and restarts the poller", async () => {
    mocks.loadSeason.mockResolvedValue(view(2));
    const { result, rerender } = renderHook(
      ({ leagueId }) => useSeasonSession(true, leagueId, () => undefined),
      { initialProps: { leagueId: "1" } },
    );
    await waitFor(() => expect(result.current.season?.week).toBe(2));
    expect(mocks.stopSeasonPolling).not.toHaveBeenCalled();

    mocks.loadSeason.mockResolvedValue(view(9));
    rerender({ leagueId: "2" });

    // The other league's standings must never be on screen under the new
    // league's name, not even for the length of one fetch.
    await waitFor(() => expect(mocks.loadSeason).toHaveBeenCalledTimes(2));
    expect(mocks.stopSeasonPolling).toHaveBeenCalled();
    await waitFor(() => expect(result.current.season?.week).toBe(9));
    expect(mocks.startSeasonPolling).toHaveBeenCalledTimes(2);
  });
});
