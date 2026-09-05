import { act, renderHook } from "@testing-library/react";
import { afterEach, beforeEach, describe, expect, it, vi } from "vitest";
import { clockListenerCount, useNow } from "./clock";

const NOW = Date.parse("2026-08-30T17:00:00Z");

beforeEach(() => {
  vi.useFakeTimers();
  vi.setSystemTime(NOW);
});

afterEach(() => {
  vi.useRealTimers();
  vi.restoreAllMocks();
});

describe("the shared wall clock", () => {
  it("runs one interval however many consumers are watching, and one value", () => {
    const start = vi.spyOn(window, "setInterval");

    const first = renderHook(() => useNow(true));
    const second = renderHook(() => useNow(true));

    expect(start).toHaveBeenCalledTimes(1);
    expect(clockListenerCount()).toBe(2);
    expect(first.result.current).toBe(NOW);
    expect(second.result.current).toBe(NOW);

    act(() => {
      vi.advanceTimersByTime(1000);
    });
    // One tick, both consumers, the same reading — the whole point of the
    // store is that two clocks on one screen cannot disagree.
    expect(first.result.current).toBe(NOW + 1000);
    expect(second.result.current).toBe(NOW + 1000);
    expect(start).toHaveBeenCalledTimes(1);
  });

  it("stops ticking once the last consumer goes away, and starts again after", () => {
    const stop = vi.spyOn(window, "clearInterval");
    const first = renderHook(() => useNow(true));
    const second = renderHook(() => useNow(true));

    first.unmount();
    expect(stop).not.toHaveBeenCalled();
    expect(clockListenerCount()).toBe(1);

    second.unmount();
    expect(stop).toHaveBeenCalledTimes(1);
    expect(clockListenerCount()).toBe(0);

    const start = vi.spyOn(window, "setInterval");
    const again = renderHook(() => useNow(true));
    expect(start).toHaveBeenCalledTimes(1);
    expect(again.result.current).toBe(NOW);
  });

  it("gives a consumer with nothing on the clock a reading without a timer", () => {
    const start = vi.spyOn(window, "setInterval");
    const { result } = renderHook(() => useNow(false));

    expect(start).not.toHaveBeenCalled();
    expect(clockListenerCount()).toBe(0);
    expect(result.current).toBe(NOW);

    // …and it is not woken for a tick it did not ask for.
    act(() => {
      vi.advanceTimersByTime(5000);
    });
    expect(result.current).toBe(NOW);
  });

  it("hands back the same reading twice for one render pass", () => {
    // `getSnapshot` used to read the system clock and write it back to the
    // store as it went. React calls it while it is deciding whether anything
    // changed — twice in a row, and again during render — so a value could
    // move between two reads of the same pass, which is the "getSnapshot
    // should be cached" loop. The refresh belongs in `subscribe`, which is an
    // effect and is allowed to write.
    const complaints = vi.spyOn(console, "error").mockImplementation(() => undefined);
    const idle = renderHook(() => useNow(false));
    const first = idle.result.current;

    vi.setSystemTime(NOW + 5000);
    idle.rerender();
    idle.rerender();

    expect(idle.result.current).toBe(first);
    expect(complaints).not.toHaveBeenCalled();
  });

  it("reads the clock afresh when it has been sitting idle", () => {
    const idle = renderHook(() => useNow(false));
    expect(idle.result.current).toBe(NOW);
    idle.unmount();

    vi.setSystemTime(NOW + 30_000);
    const later = renderHook(() => useNow(true));
    expect(later.result.current).toBe(NOW + 30_000);
  });
});
