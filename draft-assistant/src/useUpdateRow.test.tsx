import { act, renderHook, waitFor } from "@testing-library/react";
import { beforeEach, describe, expect, it, vi } from "vitest";

vi.mock("./api", async () => ({ api: (await import("./test/appHarness")).harness().api }));
vi.mock("@tauri-apps/api/app", () => ({ getVersion: () => Promise.resolve("0.2.0") }));

import { harness } from "./test/appHarness";
import { useUpdateRow } from "./useUpdateRow";

const h = harness();

/** A promise the test settles by hand, so the checking state can be seen
 *  before the answer lands. */
function pending<T>() {
  let resolve!: (value: T) => void;
  let reject!: (error: unknown) => void;
  const promise = new Promise<T>((res, rej) => {
    resolve = res;
    reject = rej;
  });
  return { promise, resolve, reject };
}

beforeEach(() => {
  h.reset();
});

// The row is the only thing in the UI that ever asks the updater. Each state a
// user can see it in, and each move between them, is walked here through the
// hook the settings menu is handed.
describe("useUpdateRow", () => {
  it("is idle until chosen, then checking, then up to date", async () => {
    const check = pending<{ current: string; available: null; notes: null }>();
    h.api.checkForUpdate.mockReturnValue(check.promise);
    const { result } = renderHook(() => useUpdateRow());
    expect(result.current.state).toEqual({ kind: "idle" });
    expect(result.current.supported).toBe(true);
    expect(h.api.checkForUpdate).not.toHaveBeenCalled();

    act(() => result.current.select());
    expect(result.current.state).toEqual({ kind: "checking" });
    expect(h.api.checkForUpdate).toHaveBeenCalledTimes(1);

    // Chosen again mid-check: nothing new is started.
    act(() => result.current.select());
    expect(h.api.checkForUpdate).toHaveBeenCalledTimes(1);

    await act(async () => {
      check.resolve({ current: "0.2.0", available: null, notes: null });
      await check.promise;
    });
    expect(result.current.state).toEqual({ kind: "current" });
  });

  it("offers the newer version, and installs it when chosen again", async () => {
    h.api.checkForUpdate.mockResolvedValue({ current: "0.2.0", available: "0.3.1", notes: "Fix" });
    const install = pending<void>();
    h.api.installUpdate.mockReturnValue(install.promise);
    const { result } = renderHook(() => useUpdateRow());

    act(() => result.current.select());
    await waitFor(() =>
      expect(result.current.state).toEqual({ kind: "available", version: "0.3.1", notes: "Fix" }),
    );

    act(() => result.current.select());
    expect(result.current.state).toEqual({ kind: "installing", version: "0.3.1" });
    expect(h.api.installUpdate).toHaveBeenCalledTimes(1);
    // A successful install restarts the app; nothing here settles, and the
    // row keeps saying so rather than snapping back to an offer.
    expect(h.api.checkForUpdate).toHaveBeenCalledTimes(1);
  });

  it("shows the backend's sentence when the check fails, and retries on the next choice", async () => {
    h.api.checkForUpdate
      .mockRejectedValueOnce(
        "Could not reach the update server. Check the connection and try again",
      )
      .mockResolvedValueOnce({ current: "0.2.0", available: null, notes: null });
    const { result } = renderHook(() => useUpdateRow());

    act(() => result.current.select());
    await waitFor(() =>
      expect(result.current.state).toEqual({
        kind: "failed",
        message: "Could not reach the update server. Check the connection and try again",
      }),
    );

    act(() => result.current.select());
    expect(result.current.state).toEqual({ kind: "checking" });
    await waitFor(() => expect(result.current.state).toEqual({ kind: "current" }));
    expect(h.api.checkForUpdate).toHaveBeenCalledTimes(2);
  });

  it("falls back to the failed state when the install itself fails", async () => {
    h.api.checkForUpdate.mockResolvedValue({ current: "0.2.0", available: "0.3.1", notes: null });
    h.api.installUpdate.mockRejectedValue(
      "The download did not match its signature, so it was not installed",
    );
    const { result } = renderHook(() => useUpdateRow());

    act(() => result.current.select());
    await waitFor(() => expect(result.current.state.kind).toBe("available"));
    act(() => result.current.select());
    await waitFor(() =>
      expect(result.current.state).toEqual({
        kind: "failed",
        message: "The download did not match its signature, so it was not installed",
      }),
    );
  });

  it("does not write into a hook that was unmounted while its check was in flight", async () => {
    const check = pending<{ current: string; available: null; notes: null }>();
    h.api.checkForUpdate.mockReturnValue(check.promise);
    const errors = vi.spyOn(console, "error").mockImplementation(() => {});
    const { result, unmount } = renderHook(() => useUpdateRow());

    act(() => result.current.select());
    unmount();
    await act(async () => {
      check.resolve({ current: "0.2.0", available: null, notes: null });
      await check.promise;
    });
    expect(errors).not.toHaveBeenCalled();
    errors.mockRestore();
  });

  it("reports no updater where the api has none, and does nothing when chosen", () => {
    const check = h.api.checkForUpdate;
    // A follower's or the preview's `api` simply lacks the method.
    (h.api as { checkForUpdate?: unknown }).checkForUpdate = undefined;
    try {
      const { result } = renderHook(() => useUpdateRow());
      expect(result.current.supported).toBe(false);
      act(() => result.current.select());
      expect(result.current.state).toEqual({ kind: "idle" });
    } finally {
      h.api.checkForUpdate = check;
    }
  });
});
