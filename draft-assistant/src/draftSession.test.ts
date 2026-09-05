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
  refreshPicks: vi.fn(),
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
  mocks.refreshPicks.mockResolvedValue(view);
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

  it("puts the old league's poller back when the switch failed", async () => {
    // Polling is stopped on the way into a switch. A switch that then fails
    // leaves the old league on screen with nothing feeding it, and the board
    // silently stops moving in the middle of a draft.
    const { said, showToast } = toasts();
    const { result } = renderHook(() => useDraftSession(showToast));
    await waitFor(() => expect(result.current.view).not.toBeNull());
    expect(result.current.polling).toBe(true);

    mocks.addLeague.mockRejectedValueOnce(new Error("no such league"));
    await act(async () => {
      await result.current.switchLeague("other");
    });

    expect(said.some((t) => /Could not switch leagues/.test(t))).toBe(true);
    expect(result.current.polling).toBe(true);
    // Once for the restore, once to put it back.
    expect(mocks.startPolling).toHaveBeenCalledTimes(2);
  });

  it("leaves the poller off when it was off before the failed switch", async () => {
    const { showToast } = toasts();
    const { result } = renderHook(() => useDraftSession(showToast));
    await waitFor(() => expect(result.current.view).not.toBeNull());
    await act(async () => {
      await result.current.togglePolling();
    });
    expect(result.current.polling).toBe(false);

    mocks.addLeague.mockRejectedValueOnce(new Error("no such league"));
    await act(async () => {
      await result.current.switchLeague("other");
    });

    expect(result.current.polling).toBe(false);
    expect(mocks.startPolling).toHaveBeenCalledTimes(1);
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

describe("when the launch itself fails", () => {
  it("shows the error's own sentence, not 'Error: Error:'", async () => {
    // `String(e)` on an Error prefixes "Error:", and the launch screen builds
    // its line from a message that already carried one.
    mocks.addLeague.mockRejectedValue(new Error("Error: Sleeper is not answering"));
    const { result } = renderHook(() => useDraftSession(() => undefined));
    await waitFor(() => expect(result.current.launchError).not.toBeNull());

    expect(result.current.launchError).toBe("Sleeper is not answering");
  });
});

describe("re-pulling the picks", () => {
  it("asks the backend again and says what came back", async () => {
    const { said, showToast } = toasts();
    const { result } = renderHook(() => useDraftSession(showToast));
    await waitFor(() => expect(result.current.view).not.toBeNull());

    await act(async () => {
      await result.current.refreshPicks();
    });

    expect(mocks.refreshPicks).toHaveBeenCalledTimes(1);
    expect(said.some((t) => /^Picks re-pulled from Sleeper/.test(t))).toBe(true);
    expect(result.current.pullingPicks).toBe(false);
  });

  it("offers the failure again rather than swallowing it", async () => {
    mocks.refreshPicks.mockRejectedValue(new Error("network timeout"));
    const said: string[] = [];
    const retries: (() => void)[] = [];
    const showToast = vi.fn((text: string, retry?: () => void) => {
      said.push(text);
      if (retry !== undefined) retries.push(retry);
    });
    const { result } = renderHook(() => useDraftSession(showToast));
    await waitFor(() => expect(result.current.view).not.toBeNull());

    await act(async () => {
      await result.current.refreshPicks();
    });

    expect(said.some((t) => /Could not re-pull the picks/.test(t))).toBe(true);
    expect(retries.length).toBeGreaterThan(0);
    expect(result.current.pullingPicks).toBe(false);
  });
});

describe("naming the service", () => {
  it("says Yahoo for a Yahoo league rather than Sleeper for everything", async () => {
    const yahoo = draftFixture();
    yahoo.league.platform = "yahoo";
    yahoo.league.league_id = "449.l.12345";
    mocks.getConfig.mockResolvedValue(restoringConfig(yahoo));
    mocks.addLeague.mockResolvedValue(yahoo);
    mocks.refreshPicks.mockResolvedValue(yahoo);
    const { said, showToast } = toasts();
    const { result } = renderHook(() => useDraftSession(showToast));
    await waitFor(() => expect(result.current.view).not.toBeNull());

    await act(async () => {
      await result.current.togglePolling();
    });
    await act(async () => {
      await result.current.togglePolling();
    });
    await act(async () => {
      await result.current.refreshPicks();
    });

    expect(said.some((t) => /polling Yahoo every 3s/.test(t))).toBe(true);
    expect(said.some((t) => /Picks re-pulled from Yahoo/.test(t))).toBe(true);
    expect(said.some((t) => /Sleeper/.test(t))).toBe(false);
  });
});
