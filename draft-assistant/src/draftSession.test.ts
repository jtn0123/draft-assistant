// The draft lifecycle on its own: what it claims happened, and what it lets a
// fresh closure from the caller do to it.

import { act, renderHook, waitFor } from "@testing-library/react";
import { beforeEach, describe, expect, it, vi } from "vitest";
import { draftFixture, restoringConfig } from "./test/appHarness";
import type { DraftView, PollHealth } from "./types";

const mocks = vi.hoisted(() => ({
  getConfig: vi.fn(),
  addLeague: vi.fn(),
  startPolling: vi.fn(),
  stopPolling: vi.fn(),
  onDraftUpdated: vi.fn(),
  onPollHealth: vi.fn(),
}));
vi.mock("./api", () => ({ api: mocks }));

import { useDraftSession } from "./draftSession";

let view: DraftView;

beforeEach(() => {
  vi.clearAllMocks();
  view = draftFixture();
  mocks.getConfig.mockResolvedValue(restoringConfig(view));
  mocks.addLeague.mockResolvedValue(view);
  mocks.startPolling.mockResolvedValue(undefined);
  mocks.stopPolling.mockResolvedValue(undefined);
  mocks.onDraftUpdated.mockImplementation((handler: (v: DraftView) => void) => {
    void handler;
    return Promise.resolve(() => undefined);
  });
  mocks.onPollHealth.mockImplementation((handler: (h: PollHealth) => void) => {
    void handler;
    return Promise.resolve(() => undefined);
  });
});

/** Everything the hook has said to the user, newest last. */
function toasts() {
  const said: string[] = [];
  const showToast = vi.fn((text: string) => void said.push(text));
  return { said, showToast };
}

describe("turning live sync on", () => {
  it("does not announce a sync that never started", async () => {
    mocks.startPolling.mockRejectedValue(new Error("no draft loaded"));
    const { said, showToast } = toasts();
    const { result } = renderHook(() => useDraftSession(showToast));
    await waitFor(() => expect(result.current.view).not.toBeNull());

    await act(async () => {
      await result.current.togglePolling();
    });

    expect(said.some((t) => /Could not turn live sync on/.test(t))).toBe(true);
    // The failure toast must be the last word, not something overwritten by a
    // cheerful one saying the opposite.
    expect(said.some((t) => /Live sync on/.test(t))).toBe(false);
    expect(result.current.polling).toBe(false);
  });

  it("announces it when it really did start", async () => {
    const { said, showToast } = toasts();
    const { result } = renderHook(() => useDraftSession(showToast));
    await waitFor(() => expect(result.current.view).not.toBeNull());
    // The restore already started it, so turn it off before turning it on.
    await act(async () => {
      await result.current.togglePolling();
    });
    await act(async () => {
      await result.current.togglePolling();
    });

    expect(said.some((t) => /Live sync on/.test(t))).toBe(true);
  });
});

describe("switching leagues", () => {
  it("does not claim the switch landed when live sync could not follow", async () => {
    const { said, showToast } = toasts();
    const { result } = renderHook(() => useDraftSession(showToast));
    await waitFor(() => expect(result.current.view).not.toBeNull());

    mocks.startPolling.mockRejectedValue(new Error("no draft loaded"));
    await act(async () => {
      await result.current.switchLeague("other");
    });

    expect(said.some((t) => /Could not turn live sync on/.test(t))).toBe(true);
    expect(said.some((t) => /^Switched to /.test(t))).toBe(false);
  });

  it("says so when the whole switch went through", async () => {
    const { said, showToast } = toasts();
    const { result } = renderHook(() => useDraftSession(showToast));
    await waitFor(() => expect(result.current.view).not.toBeNull());

    await act(async () => {
      await result.current.switchLeague("other");
    });
    expect(said.some((t) => /^Switched to /.test(t))).toBe(true);
  });
});

describe("the restore on launch", () => {
  it("runs once even when the caller hands it a new toast callback every render", async () => {
    // An inline arrow in a component is a different function on every render.
    // Nothing about that is a reason to reconnect to Sleeper.
    const { result, rerender } = renderHook(() => useDraftSession(() => undefined));
    await waitFor(() => expect(result.current.view).not.toBeNull());

    rerender();
    rerender();
    expect(mocks.getConfig).toHaveBeenCalledTimes(1);
    expect(mocks.addLeague).toHaveBeenCalledTimes(1);
    expect(mocks.startPolling).toHaveBeenCalledTimes(1);
  });
});
