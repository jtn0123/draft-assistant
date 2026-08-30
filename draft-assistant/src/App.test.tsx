import { act, render, screen, within } from "@testing-library/react";
import userEvent from "@testing-library/user-event";
import { beforeEach, describe, expect, it, vi } from "vitest";
import fixtureJson from "../public/dev-fixture.json";
import type { DraftView, PollHealth } from "./types";
import type { SeasonView } from "./season-types";

const testState = vi.hoisted(() => ({
  draftHandler: null as ((view: DraftView) => void) | null,
  healthHandler: null as ((health: PollHealth) => void) | null,
  seasonHandler: null as ((view: SeasonView) => void) | null,
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
    headshot: vi.fn(),
    startPolling: vi.fn(),
    stopPolling: vi.fn(),
    onDraftUpdated: vi.fn(),
    onPollHealth: vi.fn(),
    loadSeason: vi.fn(),
    getSeason: vi.fn(),
    refreshSeason: vi.fn(),
    startSeasonPolling: vi.fn(),
    stopSeasonPolling: vi.fn(),
    onSeasonUpdated: vi.fn(),
    setApiKey: vi.fn(),
    chatSettings: vi.fn(),
    chatSuggestions: vi.fn(),
    askClaude: vi.fn(),
  },
}));

vi.mock("./api", () => ({ api: testState.api }));

import App from "./App";

function fixture(): DraftView {
  return structuredClone(fixtureJson) as unknown as DraftView;
}

beforeEach(() => {
  vi.clearAllMocks();
  // The app opens on Season by default; these tests drive the draft board.
  // jsdom here has no storage, so give it a scratch one.
  const store = new Map<string, string>([["da.screen", "draft"]]);
  vi.stubGlobal("localStorage", {
    getItem: (k: string) => store.get(k) ?? null,
    setItem: (k: string, v: string) => void store.set(k, v),
    removeItem: (k: string) => void store.delete(k),
  });
  testState.draftHandler = null;
  testState.healthHandler = null;
  testState.seasonHandler = null;
  testState.api.startPolling.mockResolvedValue(undefined);
  testState.api.stopPolling.mockResolvedValue(undefined);
  testState.api.startSeasonPolling.mockResolvedValue(undefined);
  testState.api.stopSeasonPolling.mockResolvedValue(undefined);
  testState.api.exportState.mockResolvedValue("/tmp/draft-state.json");
  testState.api.headshot.mockResolvedValue(null);
  testState.api.chatSuggestions.mockResolvedValue([]);
  testState.api.chatSettings.mockResolvedValue({
    cli_available: false,
    provider: "api",
    has_key: false,
    key_hint: null,
    models: ["Opus 5", "Fable 5"],
    efforts: { "Opus 5": ["Off", "High"], "Fable 5": ["Low", "High"] },
    notes: {},
  });
  testState.api.onDraftUpdated.mockImplementation(async (handler) => {
    testState.draftHandler = handler;
    return () => undefined;
  });
  testState.api.onPollHealth.mockImplementation(async (handler) => {
    testState.healthHandler = handler;
    return () => undefined;
  });
  testState.api.onSeasonUpdated.mockImplementation(async (handler) => {
    testState.seasonHandler = handler;
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
    // The launch card holds the screen until the config answers.
    expect(screen.getByText("Connecting to Sleeper")).toBeInTheDocument();
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
    expect(screen.getByText(/^Live · /)).toBeInTheDocument();
    // Scoring format comes from the league's reception value (1.0 here).
    expect(screen.getByText(/^14-team full-PPR · \d+ rounds/)).toBeInTheDocument();

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
    const stale = screen.getByText("Sync stale · 2 failures");
    expect(stale.closest("span")).toHaveAttribute("title", "network timeout");

    // The first "Draft" button is the Draft/Season mode toggle, so reach into
    // the board for a row action instead.
    const rowDraft = screen.getAllByRole("button", { name: "Draft" });
    await user.click(rowDraft[rowDraft.length - 1]);
    // "Mark drafted" is also the rec-card action, so confirm inside the dialog.
    const dialog = screen.getByRole("dialog");
    await user.click(within(dialog).getByRole("button", { name: "Mark drafted" }));
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

  it("offers to reconnect rather than dropping to setup when restore fails", async () => {
    const initial = fixture();
    testState.api.getConfig.mockResolvedValue({
      my_user_id: null,
      active_league_id: initial.league.league_id,
      leagues: [{ league_id: initial.league.league_id, name: "Dynasty Warriors", season: "2026" }],
    });
    testState.api.addLeague.mockRejectedValue(new Error("request timed out"));

    render(<App />);
    // A saved league that fails to load is a connection problem, not a reason
    // to make the user re-enter their league ID.
    expect(await screen.findByRole("button", { name: "Try again" })).toBeInTheDocument();
    expect(screen.getByText(/request timed out/)).toBeInTheDocument();
    expect(screen.queryByLabelText("League ID")).not.toBeInTheDocument();
    // The launch card names the league it is bringing back.
    expect(screen.getByText("Dynasty Warriors")).toBeInTheDocument();
    expect(screen.getByText(new RegExp(`\\(${initial.league.league_id}\\)`))).toBeInTheDocument();
  });

  it("reports how many players the rebuilt board covers after a refresh", async () => {
    const user = userEvent.setup();
    const initial = fixture();
    const refreshed = fixture();
    refreshed.data_health.board_size = 312;
    testState.api.getConfig.mockResolvedValue({
      my_user_id: "browser-preview",
      active_league_id: initial.league.league_id,
      leagues: [],
    });
    testState.api.addLeague.mockResolvedValue(initial);
    testState.api.refreshData.mockResolvedValue(refreshed);

    render(<App />);
    await screen.findByText(initial.league.name);
    await user.click(screen.getByRole("button", { name: "Settings" }));
    await user.click(screen.getByRole("button", { name: /Refresh data/ }));
    expect(
      await screen.findByText("Projections refreshed — board rebuilt from 312 players"),
    ).toBeInTheDocument();
  });

  it("opens on the Season screen unless the draft board was the last choice", async () => {
    localStorage.removeItem("da.screen");
    const initial = fixture();
    testState.api.getConfig.mockResolvedValue({
      my_user_id: "browser-preview",
      active_league_id: initial.league.league_id,
      leagues: [],
    });
    testState.api.addLeague.mockResolvedValue(initial);
    testState.api.loadSeason.mockResolvedValue(seasonFixture());

    render(<App />);
    expect(await screen.findByText("vs punt_god · 122.4 – 108.9")).toBeInTheDocument();
    expect(screen.getByRole("button", { name: "Season" })).toHaveAttribute("aria-pressed", "true");
  });

  it("loads the season view when the Season tab is opened", async () => {
    const user = userEvent.setup();
    const initial = fixture();
    testState.api.getConfig.mockResolvedValue({
      my_user_id: "browser-preview",
      active_league_id: initial.league.league_id,
      leagues: [],
    });
    testState.api.addLeague.mockResolvedValue(initial);
    testState.api.loadSeason.mockResolvedValue(seasonFixture());

    render(<App />);
    await screen.findByText(initial.league.name);
    await user.click(screen.getByRole("button", { name: "Season" }));

    expect(await screen.findByText("vs punt_god · 122.4 – 108.9")).toBeInTheDocument();
    expect(testState.api.loadSeason).toHaveBeenCalledWith(false);
    // Playoff odds come through as a percentage, not a raw fraction — once in
    // the header strip and once in the standings row.
    expect(screen.getAllByText("88%")).toHaveLength(2);
  });

  it("drives the league tabs from the keyboard, as the tablist role promises", async () => {
    const user = userEvent.setup();
    const initial = fixture();
    testState.api.getConfig.mockResolvedValue({
      my_user_id: "browser-preview",
      active_league_id: initial.league.league_id,
      leagues: [],
    });
    testState.api.addLeague.mockResolvedValue(initial);
    testState.api.loadSeason.mockResolvedValue(seasonFixture());

    render(<App />);
    await screen.findByText(initial.league.name);
    await user.click(screen.getByRole("button", { name: "Season" }));
    await screen.findByText("vs punt_god · 122.4 – 108.9");

    const standings = screen.getByRole("tab", { name: "Standings" });
    expect(standings).toHaveAttribute("aria-selected", "true");
    // Only the selected tab is in the tab order.
    expect(standings).toHaveAttribute("tabindex", "0");
    expect(screen.getByRole("tab", { name: "Games" })).toHaveAttribute("tabindex", "-1");

    standings.focus();
    await user.keyboard("{ArrowRight}");
    expect(screen.getByRole("tab", { name: "Games" })).toHaveAttribute("aria-selected", "true");
    expect(screen.getByRole("tab", { name: "Games" })).toHaveFocus();

    await user.keyboard("{End}");
    expect(screen.getByRole("tab", { name: "Last season" })).toHaveAttribute(
      "aria-selected",
      "true",
    );
    // Wraps around rather than stopping at the end.
    await user.keyboard("{ArrowRight}");
    expect(standings).toHaveAttribute("aria-selected", "true");

    // The panel is announced and points back at its tab.
    const panel = screen.getByRole("tabpanel");
    expect(panel).toHaveAttribute("aria-labelledby", standings.id);
    expect(standings).toHaveAttribute("aria-controls", panel.id);
  });

  it("shows a season load failure as an error with a working retry", async () => {
    const user = userEvent.setup();
    const initial = fixture();
    testState.api.getConfig.mockResolvedValue({
      my_user_id: "browser-preview",
      active_league_id: initial.league.league_id,
      leagues: [],
    });
    testState.api.addLeague.mockResolvedValue(initial);
    testState.api.loadSeason
      .mockRejectedValueOnce(new Error("Sleeper timed out"))
      .mockResolvedValueOnce(seasonFixture());

    render(<App />);
    await screen.findByText(initial.league.name);
    await user.click(screen.getByRole("button", { name: "Season" }));

    const retry = await screen.findByRole("button", { name: "Try again" });
    // Once in the toast, once in the error block itself.
    expect(screen.getAllByText(/Sleeper timed out/)).toHaveLength(2);

    await user.click(retry);
    expect(await screen.findByText("vs punt_god · 122.4 – 108.9")).toBeInTheDocument();
    expect(testState.api.loadSeason).toHaveBeenLastCalledWith(true);
  });
});

function seasonFixture(): SeasonView {
  return {
    schema_version: "1.0",
    generated_at: 0,
    team_avatars: {},
    league: {
      league_id: "1",
      name: "Dynasty Warriors",
      season: "2026",
      total_rosters: 12,
      roster_positions: ["QB", "RB", "BN"],
      draftable_positions: ["QB", "RB"],
      scoring_settings: {},
    },
    week: 3,
    season: "2026",
    my_roster_id: 1,
    header: {
      opponent_name: "punt_god",
      my_projected: 122.4,
      opp_projected: 108.9,
      win_odds: 0.62,
      playoff_odds: 0.88,
      locks_in_ms: null,
    },
    matchup: null,
    calls: [],
    points_on_table: 0,
    waivers: [],
    waiver_budget_left: 38,
    waiver_budget_total: 100,
    standings: [
      {
        roster_id: 1,
        seed: 1,
        name: "You",
        record: "2–0",
        wins: 2,
        losses: 0,
        ties: 0,
        points_for: 250,
        projected_points: 1642,
        playoff_odds: 0.88,
        is_mine: true,
      },
    ],
    live: {
      games: [],
      windows: [],
      totals: {
        my_playing: 0,
        my_pre: 0,
        my_done: 0,
        my_live_points: 0,
        opp_live_points: 0,
      },
      next_kickoff_ms: null,
      bye_teams: [],
    },
    roster: [],
    trades: [],
    recent_trades: [],
    activity: [],
    last_season: [],
    trends: { series: [], changes: [] },
    data_health: { fetched_at: 0, warnings: [] },
  };
}
