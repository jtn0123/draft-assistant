import { act, render, screen } from "@testing-library/react";
import userEvent from "@testing-library/user-event";
import { beforeEach, describe, expect, it, vi } from "vitest";
import fixtureJson from "../public/dev-fixture.json";
import type { DraftView, PollHealth } from "./types";

const testState = vi.hoisted(() => ({
  draftHandler: null as ((view: DraftView) => void) | null,
  errorHandler: null as ((error: unknown) => void) | null,
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

// The backend stamps every build with a strictly increasing `seq`, and the UI
// drops anything not newer than what it has rendered. Mirror that here so each
// synthetic view is genuinely newer than the last.
let nextSeq = 0;

function fixture(): DraftView {
  const view = structuredClone(fixtureJson) as unknown as DraftView;
  view.seq = ++nextSeq;
  return view;
}

beforeEach(() => {
  vi.clearAllMocks();
  testState.draftHandler = null;
  testState.errorHandler = null;
  testState.healthHandler = null;
  nextSeq = 0;
  testState.api.startPolling.mockResolvedValue(undefined);
  testState.api.stopPolling.mockResolvedValue(undefined);
  testState.api.exportState.mockResolvedValue("/tmp/draft-state.json");
  testState.api.onDraftUpdated.mockImplementation(async (handler, onError) => {
    testState.draftHandler = handler;
    testState.errorHandler = onError ?? null;
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

  it("disables Undo until there is a manual pick to undo", async () => {
    const initial = fixture();
    initial.draft.manual_picks_active = false;
    testState.api.getConfig.mockResolvedValue({
      my_user_id: "browser-preview",
      active_league_id: initial.league.league_id,
      leagues: [],
    });
    testState.api.addLeague.mockResolvedValue(initial);

    render(<App />);
    expect(await screen.findByText(initial.league.name)).toBeInTheDocument();
    expect(screen.getByRole("button", { name: "Undo" })).toBeDisabled();

    const withManual = fixture();
    withManual.draft.manual_picks_active = true;
    act(() => testState.draftHandler?.(withManual));
    expect(screen.getByRole("button", { name: "Undo" })).toBeEnabled();
  });

  it("ignores a view that is older than what is already rendered", async () => {
    const initial = fixture();
    testState.api.getConfig.mockResolvedValue({
      my_user_id: "browser-preview",
      active_league_id: initial.league.league_id,
      leagues: [],
    });
    testState.api.addLeague.mockResolvedValue(initial);

    render(<App />);
    expect(await screen.findByText(initial.league.name)).toBeInTheDocument();

    const newer = fixture();
    newer.league.name = "Newer";
    act(() => testState.draftHandler?.(newer));
    expect(screen.getByText("Newer")).toBeInTheDocument();

    // A poll that started earlier but landed later must not win.
    const stale = fixture();
    stale.league.name = "Stale poll result";
    stale.seq = newer.seq - 1;
    act(() => testState.draftHandler?.(stale));
    expect(screen.queryByText("Stale poll result")).not.toBeInTheDocument();
    expect(screen.getByText("Newer")).toBeInTheDocument();
  });

  it("reports a rejected live update instead of dropping it silently", async () => {
    const initial = fixture();
    testState.api.getConfig.mockResolvedValue({
      my_user_id: "browser-preview",
      active_league_id: initial.league.league_id,
      leagues: [],
    });
    testState.api.addLeague.mockResolvedValue(initial);

    render(<App />);
    expect(await screen.findByText(initial.league.name)).toBeInTheDocument();

    // The schema guard threw inside the event callback. Thrown there it would
    // vanish; routed here it must reach the user.
    act(() => {
      testState.errorHandler?.(
        new Error("Incompatible draft data: expected schema 1.2, received 1.1"),
      );
    });
    expect(
      screen.getByText(/Live update rejected: Incompatible draft data/),
    ).toBeInTheDocument();
    expect(screen.getByText(initial.league.name)).toBeInTheDocument();
  });

  it("cancels a pending pick when that player is drafted by someone else", async () => {
    const user = userEvent.setup();
    const initial = fixture();
    testState.api.getConfig.mockResolvedValue({
      my_user_id: "browser-preview",
      active_league_id: initial.league.league_id,
      leagues: [],
    });
    testState.api.addLeague.mockResolvedValue(initial);

    render(<App />);
    expect(await screen.findByText(initial.league.name)).toBeInTheDocument();

    await user.click(screen.getAllByRole("button", { name: "Draft" })[0]);
    expect(screen.getByRole("button", { name: "Confirm" })).toBeInTheDocument();

    // Live sync reports Chris Olave gone while the modal sits open.
    const taken = fixture();
    taken.available = taken.available.filter((p) => p.player_id !== "8144");
    taken.recommendations = [];
    act(() => testState.draftHandler?.(taken));

    expect(screen.queryByRole("button", { name: "Confirm" })).not.toBeInTheDocument();
    expect(
      screen.getByText(/Chris Olave was drafted by another team/),
    ).toBeInTheDocument();
    expect(testState.api.recordManualPick).not.toHaveBeenCalled();
  });

  it("keeps a failed action on screen until it is dismissed", async () => {
    const user = userEvent.setup();
    const initial = fixture();
    testState.api.getConfig.mockResolvedValue({
      my_user_id: "browser-preview",
      active_league_id: initial.league.league_id,
      leagues: [],
    });
    testState.api.addLeague.mockResolvedValue(initial);
    testState.api.recordManualPick.mockRejectedValue(new Error("player already drafted"));

    render(<App />);
    expect(await screen.findByText(initial.league.name)).toBeInTheDocument();

    await user.click(screen.getAllByRole("button", { name: "Draft" })[0]);
    await user.click(screen.getByRole("button", { name: "Confirm" }));

    const alert = await screen.findByRole("alert");
    expect(alert).toHaveTextContent("player already drafted");
    expect(alert).not.toHaveTextContent("Error:");

    await user.click(screen.getByRole("button", { name: "Dismiss message" }));
    expect(screen.queryByRole("alert")).not.toBeInTheDocument();
  });

  it("submits the setup form with Enter", async () => {
    const user = userEvent.setup();
    const initial = fixture();
    testState.api.getConfig.mockResolvedValue({
      my_user_id: null,
      active_league_id: null,
      leagues: [],
    });
    testState.api.setMyUsername.mockResolvedValue("872674602265051136");
    testState.api.addLeague.mockResolvedValue(initial);

    render(<App />);
    await user.type(await screen.findByLabelText("Sleeper username"), "mcsleeper26");
    await user.type(screen.getByLabelText("League ID"), "1389710366300200960{Enter}");
    expect(await screen.findByText(initial.league.name)).toBeInTheDocument();
    expect(testState.api.setMyUsername).toHaveBeenCalledWith("mcsleeper26");
    expect(testState.api.addLeague).toHaveBeenCalledWith("1389710366300200960");
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
    expect(await screen.findByText("league unavailable")).toBeInTheDocument();
  });
});
