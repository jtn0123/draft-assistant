import { act, render, screen } from "@testing-library/react";
import userEvent from "@testing-library/user-event";
import { beforeEach, describe, expect, it, vi } from "vitest";
import fixtureJson from "../public/dev-fixture.json";
import type { DraftView, PollHealth } from "./types";

const testState = vi.hoisted(() => ({
  draftHandler: null as ((view: DraftView) => void) | null,
  healthHandler: null as ((health: PollHealth) => void) | null,
  api: {
    addLeague: vi.fn(),
    setMyUsername: vi.fn(),
    getConfig: vi.fn(),
    getState: vi.fn(),
    refreshPicks: vi.fn(),
    refreshData: vi.fn(),
    recordManualPick: vi.fn(),
    undoManualPick: vi.fn(),
    exportState: vi.fn(),
    startPolling: vi.fn(),
    stopPolling: vi.fn(),
    onDraftUpdated: vi.fn(),
    onPollHealth: vi.fn(),
  },
}));

vi.mock("./api", () => ({ api: testState.api }));

import App from "./App";

function fixture(): DraftView {
  return structuredClone(fixtureJson) as unknown as DraftView;
}

beforeEach(() => {
  vi.clearAllMocks();
  testState.draftHandler = null;
  testState.healthHandler = null;
  testState.api.startPolling.mockResolvedValue(undefined);
  testState.api.stopPolling.mockResolvedValue(undefined);
  testState.api.exportState.mockResolvedValue("/tmp/draft-state.json");
  testState.api.onDraftUpdated.mockImplementation(async (handler) => {
    testState.draftHandler = handler;
    return () => undefined;
  });
  testState.api.onPollHealth.mockImplementation(async (handler) => {
    testState.healthHandler = handler;
    return () => undefined;
  });
});

describe("App live workflow", () => {
  it("shows setup only after confirming there is no saved league", async () => {
    testState.api.getConfig.mockResolvedValue({
      my_user_id: null,
      active_league_id: null,
      leagues: [],
    });

    render(<App />);
    expect(screen.getByText("Loading your league…")).toBeInTheDocument();
    expect(await screen.findByLabelText("League ID")).toBeInTheDocument();
  });

  it("loads live state, exposes failures, and supports manual pick and undo", async () => {
    const user = userEvent.setup();
    const initial = fixture();
    const afterPick = fixture();
    afterPick.available = afterPick.available.slice(1);
    afterPick.draft.total_picks_made += 1;
    testState.api.getConfig.mockResolvedValue({
      my_user_id: "browser-preview",
      active_league_id: initial.league.league_id,
      leagues: [],
    });
    testState.api.addLeague.mockResolvedValue(initial);
    testState.api.recordManualPick.mockResolvedValue(afterPick);
    testState.api.undoManualPick.mockResolvedValue(initial);

    render(<App />);
    expect(await screen.findByText(initial.league.name)).toBeInTheDocument();
    expect(screen.getByRole("button", { name: "● Live sync on" })).toBeInTheDocument();

    const liveUpdate = fixture();
    liveUpdate.league.name = "League updated by poll";
    act(() => testState.draftHandler?.(liveUpdate));
    expect(screen.getByText("League updated by poll")).toBeInTheDocument();

    act(() => {
      testState.healthHandler?.({
        last_success_at: initial.generated_at,
        consecutive_failures: 2,
        last_error: "network timeout",
      });
    });
    expect(screen.getByRole("button", { name: "● Sync stale · 2 failures" }))
      .toHaveAttribute("title", "network timeout");

    await user.click(screen.getAllByRole("button", { name: "Draft" })[0]);
    await user.click(screen.getByRole("button", { name: "Confirm" }));
    expect(testState.api.recordManualPick).toHaveBeenCalledTimes(1);

    await user.click(screen.getByRole("button", { name: "Undo" }));
    expect(testState.api.undoManualPick).toHaveBeenCalledTimes(1);
  });

  it("shows a setup error when a league cannot be loaded", async () => {
    const user = userEvent.setup();
    testState.api.getConfig.mockResolvedValue({
      my_user_id: null,
      active_league_id: null,
      leagues: [],
    });
    testState.api.addLeague.mockRejectedValue(new Error("league unavailable"));

    render(<App />);
    await user.type(await screen.findByLabelText("League ID"), "123456789012345");
    await user.click(screen.getByRole("button", { name: "Load league" }));
    expect(await screen.findByText("Error: league unavailable")).toBeInTheDocument();
  });
});
