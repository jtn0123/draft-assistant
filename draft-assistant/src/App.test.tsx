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
    evaluateTrade: vi.fn(),
    exportState: vi.fn(),
    saveChatSession: vi.fn(),
    listChatSessions: vi.fn(),
    loadChatSession: vi.fn(),
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
  // These tests are about App's wiring, not the board's size. The real dump
  // carries ~370 available players, and rendering 200 rows several times per
  // test pushed the heavier cases past the 5 s timeout on CI's slower runner.
  view.available = view.available.slice(0, 40);
  return view;
}

beforeEach(() => {
  vi.clearAllMocks();
  window.localStorage.clear();
  testState.draftHandler = null;
  testState.errorHandler = null;
  testState.healthHandler = null;
  nextSeq = 0;
  testState.api.startPolling.mockResolvedValue(undefined);
  testState.api.listChatSessions.mockResolvedValue([]);
  testState.api.saveChatSession.mockResolvedValue("");
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

describe("pick numbering toggle", () => {
  it("switches every pick in the page between overall and round.pick", async () => {
    const user = userEvent.setup();
    const initial = fixture();
    initial.draft.teams = 14;
    initial.draft.current_pick = 55;
    initial.draft.current_round = 4;
    initial.draft.my_next_picks = [55, 58, 83];
    testState.api.getConfig.mockResolvedValue({
      my_user_id: "browser-preview",
      active_league_id: initial.league.league_id,
      leagues: [],
    });
    testState.api.addLeague.mockResolvedValue(initial);

    render(<App />);
    await screen.findByText(initial.league.name);

    const toggle = screen.getByLabelText("Toggle pick numbering");
    expect(toggle).toHaveTextContent("#55");
    // Your upcoming picks, in the clock banner, as Sleeper numbers them.
    expect(screen.getByText("55 · 58 · 83")).toBeInTheDocument();

    await user.click(toggle);
    expect(toggle).toHaveTextContent("4.13");
    expect(screen.getByText("4.13 · 5.2 · 6.13")).toBeInTheDocument();
    expect(window.localStorage.getItem("draft-assistant.pick-style")).toBe("round");

    await user.click(toggle);
    expect(screen.getByText("55 · 58 · 83")).toBeInTheDocument();
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
    // Pin the sync age rather than relying on how recently the checked-in
    // fixture was captured.
    act(() => {
      testState.healthHandler?.({
        last_success_at: Math.floor(Date.now() / 1000),
        consecutive_failures: 0,
        last_error: null,
      });
    });
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

    // Live sync reports the pending player gone while the modal sits open.
    const taken = fixture();
    const pending = initial.available[0];
    taken.available = taken.available.filter((p) => p.player_id !== pending.player_id);
    taken.recommendations = [];
    act(() => testState.draftHandler?.(taken));

    expect(screen.queryByRole("button", { name: "Confirm" })).not.toBeInTheDocument();
    expect(
      screen.getByText(new RegExp(`${pending.name} was drafted by another team`)),
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

  // Dogfood ISSUE-001: the success notice fired even when starting the poller
  // had just failed, so the toast said sync was on while the pill said off and
  // the real error was overwritten.
  it("reports a failed live-sync start instead of claiming sync is on", async () => {
    const user = userEvent.setup();
    const initial = fixture();
    testState.api.getConfig.mockResolvedValue({
      my_user_id: "browser-preview",
      active_league_id: initial.league.league_id,
      leagues: [],
    });
    testState.api.addLeague.mockResolvedValue(initial);
    testState.api.startPolling.mockRejectedValue(new Error("live sync is unavailable here"));

    render(<App />);
    expect(await screen.findByText(initial.league.name)).toBeInTheDocument();
    await user.click(screen.getByRole("button", { name: "Dismiss message" }));

    await user.click(screen.getByRole("button", { name: "○ Live sync off" }));
    expect(await screen.findByRole("alert")).toHaveTextContent("live sync is unavailable here");
    expect(screen.queryByText(/Live sync on — polling Sleeper/)).not.toBeInTheDocument();
    expect(screen.getByRole("button", { name: "○ Live sync off" })).toBeInTheDocument();
  });

  // Dogfood ISSUE-004: the launch failure was prepared but never rendered,
  // because the failure bar only existed on the main screen.
  it("says why the saved league failed to load instead of a bare setup screen", async () => {
    const initial = fixture();
    testState.api.getConfig.mockResolvedValue({
      my_user_id: null,
      active_league_id: initial.league.league_id,
      leagues: [],
    });
    testState.api.addLeague.mockRejectedValue(new Error("Sleeper is unreachable"));

    render(<App />);
    expect(await screen.findByLabelText("League ID")).toBeInTheDocument();
    expect(screen.getByRole("alert")).toHaveTextContent("Sleeper is unreachable");
    // Retrying is one click, not retyping an 19-digit id.
    expect(screen.getByLabelText("League ID")).toHaveValue(initial.league.league_id);
    expect(testState.api.addLeague).toHaveBeenCalledTimes(1);
  });

  it("retries a launch that stalled on the network before giving up", async () => {
    const initial = fixture();
    testState.api.getConfig.mockResolvedValue({
      my_user_id: "browser-preview",
      active_league_id: initial.league.league_id,
      leagues: [],
    });
    testState.api.addLeague
      .mockRejectedValueOnce(new Error("client error (Connect): operation timed out"))
      .mockResolvedValue(initial);

    render(<App />);
    expect(await screen.findByText(initial.league.name, {}, { timeout: 4000 })).toBeInTheDocument();
    expect(testState.api.addLeague).toHaveBeenCalledTimes(2);
    expect(screen.queryByRole("alert")).not.toBeInTheDocument();
  }, 8000);

  // Dogfood ISSUE-011: every row still offered an enabled Draft button once
  // the draft was over, and the pick was only refused after Confirm.
  it("disables drafting once the draft is complete", async () => {
    const user = userEvent.setup();
    const initial = fixture();
    initial.draft.status = "complete";
    testState.api.getConfig.mockResolvedValue({
      my_user_id: "browser-preview",
      active_league_id: initial.league.league_id,
      leagues: [],
    });
    testState.api.addLeague.mockResolvedValue(initial);

    render(<App />);
    expect(await screen.findByText(initial.league.name)).toBeInTheDocument();
    // A complete draft opens on the season; the board is one switch away.
    await user.click(screen.getByRole("button", { name: "Draft screen" }));
    for (const button of screen.getAllByRole("button", { name: /^Draft$/ })) {
      expect(button).toBeDisabled();
    }
  });

  // Dogfood ISSUE-008: with the feed dead the pill stayed green and the age
  // froze at whatever it said when the last update arrived — the one number
  // that reports staleness stopped moving exactly when data went stale.
  it("keeps the sync age moving and flags a feed that has gone quiet", async () => {
    vi.useFakeTimers();
    try {
      const initial = fixture();
      testState.api.getConfig.mockResolvedValue({
        my_user_id: "browser-preview",
        active_league_id: initial.league.league_id,
        leagues: [],
      });
      testState.api.addLeague.mockResolvedValue(initial);

      render(<App />);
      await act(async () => {
        await vi.advanceTimersByTimeAsync(0);
      });
      act(() => {
        testState.healthHandler?.({
          last_success_at: Math.floor(Date.now() / 1000),
          consecutive_failures: 0,
          last_error: null,
        });
      });
      expect(screen.getByText("Last sync 0s ago")).toBeInTheDocument();
      expect(screen.getByRole("button", { name: "● Live sync on" })).toBeInTheDocument();

      await act(async () => {
        await vi.advanceTimersByTimeAsync(45_000);
      });
      expect(screen.getByText("Last sync 45s ago")).toBeInTheDocument();
      expect(screen.getByRole("button", { name: /Sync stale/ })).toBeInTheDocument();
    } finally {
      vi.useRealTimers();
    }
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

describe("draft and season screens", () => {
  function load(initial: DraftView) {
    testState.api.getConfig.mockResolvedValue({
      my_user_id: "browser-preview",
      active_league_id: initial.league.league_id,
      leagues: [],
    });
    testState.api.addLeague.mockResolvedValue(initial);
  }

  it("shows the draft while it is on, with the season a switch away", async () => {
    const user = userEvent.setup();
    load(fixture());
    render(<App />);
    await screen.findByText("YOU ARE ON THE CLOCK");
    expect(screen.getByRole("button", { name: "Draft screen" })).toHaveAttribute("aria-pressed", "true");

    await user.click(screen.getByRole("button", { name: "Season screen" }));
    expect(screen.queryByText("YOU ARE ON THE CLOCK")).not.toBeInTheDocument();
    expect(screen.getByText(/No week on the calendar yet/)).toBeInTheDocument();
    // Draft-only controls leave with the draft.
    expect(screen.queryByRole("button", { name: "Undo" })).not.toBeInTheDocument();
    expect(screen.queryByLabelText("Toggle pick numbering")).not.toBeInTheDocument();
    expect(screen.getByRole("heading", { level: 2, name: "My roster" })).toBeInTheDocument();
    expect(screen.queryByRole("heading", { name: "Tier alerts" })).not.toBeInTheDocument();
  });

  it("opens on the season once the draft is complete and remembers a switch back", async () => {
    const user = userEvent.setup();
    const initial = fixture();
    initial.draft.status = "complete";
    load(initial);
    render(<App />);
    await screen.findByText(initial.league.name);
    expect(screen.getByRole("button", { name: "Season screen" })).toHaveAttribute("aria-pressed", "true");
    expect(screen.queryByText("YOU ARE ON THE CLOCK")).not.toBeInTheDocument();

    await user.click(screen.getByRole("button", { name: "Draft screen" }));
    expect(screen.getByRole("button", { name: "Undo" })).toBeInTheDocument();
    expect(
      window.localStorage.getItem(`draft-assistant.view-mode:${initial.draft.draft_id}`),
    ).toBe("draft");
  });
});
