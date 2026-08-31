// The season lifecycle on its own: load once, poll while showing, stop when
// hidden, surface a failure, and recover on retry.

import { act, renderHook, waitFor } from "@testing-library/react";
import { beforeEach, describe, expect, it, vi } from "vitest";
import type { SeasonView } from "./season-types";

const mocks = vi.hoisted(() => ({
  loadSeason: vi.fn(),
  startSeasonPolling: vi.fn(),
  stopSeasonPolling: vi.fn(),
  onSeasonUpdated: vi.fn(),
}));
vi.mock("./api", () => ({ api: mocks }));

import { useSeasonSession } from "./session";

const view = (week: number) => ({ schema_version: "1.0", week }) as unknown as SeasonView;

let pushUpdate: ((v: SeasonView) => void) | null = null;

beforeEach(() => {
  vi.clearAllMocks();
  pushUpdate = null;
  mocks.startSeasonPolling.mockResolvedValue(undefined);
  mocks.stopSeasonPolling.mockResolvedValue(undefined);
  mocks.onSeasonUpdated.mockImplementation(async (handler: (v: SeasonView) => void) => {
    pushUpdate = handler;
    return () => undefined;
  });
});

describe("useSeasonSession", () => {
  it("fetches nothing until the screen is showing and a league is loaded", () => {
    const { rerender } = renderHook(
      ({ active, ready }) => useSeasonSession(active, ready, () => undefined),
      { initialProps: { active: false, ready: true } },
    );
    expect(mocks.loadSeason).not.toHaveBeenCalled();

    rerender({ active: true, ready: false });
    expect(mocks.loadSeason).not.toHaveBeenCalled();
    expect(mocks.startSeasonPolling).not.toHaveBeenCalled();
  });

  it("loads once and starts polling when the screen opens", async () => {
    mocks.loadSeason.mockResolvedValue(view(2));
    const { result, rerender } = renderHook(
      ({ active }) => useSeasonSession(active, true, () => undefined),
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
      ({ active }) => useSeasonSession(active, true, () => undefined),
      { initialProps: { active: true } },
    );
    await waitFor(() => expect(result.current.season).not.toBeNull());

    rerender({ active: false });
    await waitFor(() => expect(mocks.stopSeasonPolling).toHaveBeenCalled());
  });

  it("applies pushed live updates", async () => {
    mocks.loadSeason.mockResolvedValue(view(2));
    const { result } = renderHook(() => useSeasonSession(true, true, () => undefined));
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
      () => useSeasonSession(true, true, () => undefined),
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
    const { result } = renderHook(() => useSeasonSession(true, true, onError));

    await waitFor(() => expect(result.current.error).toMatch(/Sleeper timed out/));
    expect(result.current.season).toBeNull();
    expect(onError).toHaveBeenCalledTimes(1);
  });

  it("retry clears the error and forces a fresh fetch", async () => {
    mocks.loadSeason.mockRejectedValueOnce(new Error("down")).mockResolvedValue(view(5));
    // A stable callback, so the load effect does not re-fire on every render
    // and race the retry with a second automatic fetch.
    const quiet = () => undefined;
    const { result } = renderHook(() => useSeasonSession(true, true, quiet));
    await waitFor(() => expect(result.current.error).not.toBeNull());

    // Retry drives two state updates; let React flush both before reading them.
    await act(async () => {
      result.current.retry();
    });
    await waitFor(() => expect(result.current.season?.week).toBe(5));
    expect(result.current.error).toBeNull();
    // force=true: a retry must bypass the cache that just failed.
    expect(mocks.loadSeason).toHaveBeenLastCalledWith(true);
  });
});
