// Recording a pick by hand, and what a second press of the button does.

import { act, renderHook, waitFor } from "@testing-library/react";
import { beforeEach, describe, expect, it, vi } from "vitest";
import { draftFixture } from "./test/appHarness";
import type { DraftView } from "./types";

const mocks = vi.hoisted(() => ({ recordManualPick: vi.fn() }));
vi.mock("./api", () => ({ api: mocks }));

import { useMarkDrafted } from "./markDrafted";

let view: DraftView;

beforeEach(() => {
  vi.clearAllMocks();
  view = draftFixture(8);
});

/** Everything the hook said to the user, newest last. */
function toasts() {
  const said: string[] = [];
  const showToast = vi.fn((text: string) => void said.push(text));
  return { said, showToast };
}

describe("marking a player drafted", () => {
  it("sends one pick however many times the button is pressed", async () => {
    // The failure: React batches state, so two clicks inside one frame both
    // read `drafting` as false and both sent the pick.
    let release: (v: DraftView) => void = () => undefined;
    mocks.recordManualPick.mockReturnValue(
      new Promise<DraftView>((resolve) => {
        release = resolve;
      }),
    );
    const applyView = vi.fn();
    const { said, showToast } = toasts();
    const { result } = renderHook(() => useMarkDrafted(applyView, showToast, null));

    act(() => result.current.ask("4046", "Patrick Mahomes"));
    expect(result.current.confirm?.name).toBe("Patrick Mahomes");

    act(() => {
      result.current.confirmDraft();
      result.current.confirmDraft();
    });
    expect(mocks.recordManualPick).toHaveBeenCalledTimes(1);
    expect(result.current.drafting).toBe(true);

    await act(async () => {
      release(view);
      await Promise.resolve();
    });
    await waitFor(() => expect(result.current.confirm).toBeNull());
    expect(applyView).toHaveBeenCalledWith(view);
    expect(said).toEqual([]);
  });

  it("treats a pick we already made as done, not as a failure", async () => {
    mocks.recordManualPick.mockResolvedValueOnce(view);
    const { said, showToast } = toasts();
    const { result } = renderHook(() => useMarkDrafted(vi.fn(), showToast, null));

    act(() => result.current.ask("4046", "Patrick Mahomes"));
    await act(async () => {
      result.current.confirmDraft();
      await Promise.resolve();
    });

    // The retry the toast would have offered, or a stale second window: the
    // backend refuses because the player is already off the board, which is
    // the one failure that means the same thing as success.
    mocks.recordManualPick.mockRejectedValueOnce(new Error("player already drafted"));
    act(() => result.current.ask("4046", "Patrick Mahomes"));
    await act(async () => {
      result.current.confirmDraft();
      await Promise.resolve();
    });

    expect(said).toEqual([]);
    expect(result.current.confirm).toBeNull();
  });

  it("still says so when the pick genuinely failed", async () => {
    mocks.recordManualPick.mockRejectedValue(new Error("no draft loaded"));
    const { said, showToast } = toasts();
    const { result } = renderHook(() => useMarkDrafted(vi.fn(), showToast, null));

    act(() => result.current.ask("4046", "Patrick Mahomes"));
    await act(async () => {
      result.current.confirmDraft();
      await Promise.resolve();
    });

    expect(said).toHaveLength(1);
    expect(said[0]).toMatch(/Could not mark Patrick Mahomes as drafted/);
  });

  it("refuses a player already drafted when this window never recorded them", async () => {
    mocks.recordManualPick.mockRejectedValue(new Error("player already drafted"));
    const { said, showToast } = toasts();
    const { result } = renderHook(() => useMarkDrafted(vi.fn(), showToast, null));

    act(() => result.current.ask("99", "Someone Else"));
    await act(async () => {
      result.current.confirmDraft();
      await Promise.resolve();
    });

    expect(said).toHaveLength(1);
  });

  it("tells a follower who records the picks instead of opening a dialog", () => {
    const { said, showToast } = toasts();
    const { result } = renderHook(() => useMarkDrafted(vi.fn(), showToast, "Justin's Mac"));

    act(() => result.current.ask("4046", "Patrick Mahomes"));

    expect(result.current.confirm).toBeNull();
    expect(said).toEqual(["Justin's Mac records the picks"]);
    expect(mocks.recordManualPick).not.toHaveBeenCalled();
  });
});
