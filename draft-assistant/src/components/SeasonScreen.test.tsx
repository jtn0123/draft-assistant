import { act, render, screen } from "@testing-library/react";
import { afterEach, describe, expect, it, vi } from "vitest";
import type { SeasonView, SourceHealth } from "../season-types";
import { SeasonScreen } from "./SeasonScreen";

const NOW = () => Math.floor(Date.now() / 1000);

function fresh(): SourceHealth {
  return {
    matchups: { last_success_secs: NOW() - 5, error: null },
    scores: { last_success_secs: NOW() - 5, error: null },
    rosters: { last_success_secs: NOW() - 5, error: null },
  };
}

function view(overrides: Partial<SeasonView> = {}): SeasonView {
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
    standings: [],
    live: {
      games: [],
      windows: [],
      totals: { my_playing: 0, my_pre: 0, my_done: 0, my_live_points: 0, opp_live_points: 0 },
      next_kickoff_ms: null,
      bye_teams: [],
    },
    roster: [],
    trades: [],
    recent_trades: [],
    activity: [],
    last_season: [],
    trends: { series: [], changes: [] },
    data_health: { fetched_at: NOW(), warnings: [], sources: fresh() },
    ...overrides,
  };
}

afterEach(() => {
  vi.useRealTimers();
});

describe("the live badge", () => {
  it("calls itself live and dates every source when all three are current", () => {
    render(<SeasonScreen view={view()} />);
    const badge = screen.getByText(/^Live · /);
    expect(badge).toHaveClass("pill-live");
    const title = badge.getAttribute("title") ?? "";
    expect(title).toContain("Matchups: 5 seconds ago");
    expect(title).toContain("Scores: 5 seconds ago");
    expect(title).toContain("Rosters: 5 seconds ago");
  });

  it("names the source that is behind and says why, without going fully red", () => {
    const sources = fresh();
    sources.rosters = { last_success_secs: NOW() - 720, error: "timeout" };
    render(
      <SeasonScreen view={view({ data_health: { fetched_at: NOW(), warnings: [], sources } })} />,
    );

    const badge = screen.getByText("Live · rosters behind");
    // Warning-ish, not the all-clear: two feeds are still arriving.
    expect(badge).toHaveClass("pill-stale");
    const title = badge.getAttribute("title") ?? "";
    expect(title).toContain("Rosters: failing for 12 minutes (timeout)");
    expect(title).toContain("Scores: 5 seconds ago");
    expect(title).toContain("Matchups: 5 seconds ago");
  });

  it("writes the source breakdown under the badge once something is behind", () => {
    const sources = fresh();
    sources.rosters = { last_success_secs: NOW() - 720, error: "timeout" };
    render(
      <SeasonScreen view={view({ data_health: { fetched_at: NOW(), warnings: [], sources } })} />,
    );

    // No hovering required: which feed, and how far behind, is on the page.
    expect(screen.getByText("Rosters: failing for 12 minutes (timeout)")).toBeInTheDocument();
    expect(screen.getByText("Scores: 5 seconds ago")).toBeInTheDocument();
    expect(screen.getByText("Matchups: 5 seconds ago")).toBeInTheDocument();
  });

  it("keeps the breakdown out of the way while every source is current", () => {
    render(<SeasonScreen view={view()} />);
    expect(screen.queryByText("Scores: 5 seconds ago")).not.toBeInTheDocument();
  });

  it("stops claiming to be live when nothing at all is arriving", () => {
    const sources: SourceHealth = {
      matchups: { last_success_secs: NOW() - 900, error: "503" },
      scores: { last_success_secs: NOW() - 900, error: "503" },
      rosters: { last_success_secs: NOW() - 900, error: "503" },
    };
    render(
      <SeasonScreen view={view({ data_health: { fetched_at: NOW(), warnings: [], sources } })} />,
    );
    expect(screen.getByText("Not updating")).toHaveClass("pill-stale");
  });

  // The badge is the one thing on the screen whose job is to notice an
  // absence, so it has to keep moving when nothing else does.
  it("stops calling itself live when time passes and no new data arrives", async () => {
    vi.useFakeTimers();
    render(<SeasonScreen view={view()} />);
    expect(screen.getByText(/^Live · /)).toHaveClass("pill-live");

    // Well past the point where all three sources are behind, with no re-render
    // from new data — only the badge's own heartbeat.
    await act(async () => {
      await vi.advanceTimersByTimeAsync(120_000);
    });
    expect(screen.getByText("Not updating")).toHaveClass("pill-stale");
  });

  it("goes stale on the overall stamp too, when there is no per-source detail", async () => {
    vi.useFakeTimers();
    render(<SeasonScreen view={view({ data_health: { fetched_at: NOW(), warnings: [] } })} />);
    expect(screen.getByText(/^Live · /)).toHaveClass("pill-live");

    await act(async () => {
      await vi.advanceTimersByTimeAsync(120_000);
    });
    expect(screen.getByText("Not updating")).toHaveClass("pill-stale");
  });

  it("falls back to the overall stamp for a view with no per-source detail", () => {
    render(<SeasonScreen view={view({ data_health: { fetched_at: NOW() - 4, warnings: [] } })} />);
    const badge = screen.getByText("Live · 4s ago");
    expect(badge).toHaveClass("pill-live");
    expect(badge).not.toHaveAttribute("title");
  });
});

describe("a failing live poll", () => {
  it("says the feed is failing, and why, even while the stamps still look fresh", () => {
    render(
      <SeasonScreen
        view={view()}
        pollHealth={{
          last_success_at: NOW() - 300,
          consecutive_failures: 3,
          last_error: "scores: request failed",
        }}
      />,
    );
    // The badge stops vouching for the data even though every source stamp is
    // five seconds old — the poller knows better than the timestamps do.
    expect(screen.getByText("Not updating")).toHaveClass("pill-stale");
    expect(screen.getByText(/The last 3 tries to get new scores failed/)).toBeInTheDocument();
    expect(screen.getByText(/5 minutes ago/)).toBeInTheDocument();
    expect(screen.getByText(/scores: request failed/)).toBeInTheDocument();
  });

  it("counts a single failure in the singular, and says when nothing has arrived yet", () => {
    render(
      <SeasonScreen
        view={view()}
        pollHealth={{ last_success_at: null, consecutive_failures: 1, last_error: null }}
      />,
    );
    expect(
      screen.getByText("The last try to get new scores failed — no scores have come through yet"),
    ).toBeInTheDocument();
  });

  it("says nothing at all while the poll is getting through", () => {
    render(
      <SeasonScreen
        view={view()}
        pollHealth={{ last_success_at: NOW(), consecutive_failures: 0, last_error: null }}
      />,
    );
    expect(screen.getByText(/^Live · /)).toHaveClass("pill-live");
    expect(screen.queryByText(/to get new scores failed/)).not.toBeInTheDocument();
  });

  it("keeps the stale badge but adds the reason when the sources are behind too", () => {
    const sources = fresh();
    sources.scores = { last_success_secs: NOW() - 600, error: "timeout" };
    sources.matchups = { last_success_secs: NOW() - 600, error: "timeout" };
    sources.rosters = { last_success_secs: NOW() - 600, error: "timeout" };
    render(
      <SeasonScreen
        view={view({ data_health: { fetched_at: NOW() - 600, warnings: [], sources } })}
        pollHealth={{
          last_success_at: NOW() - 600,
          consecutive_failures: 2,
          last_error: "timeout",
        }}
      />,
    );
    // One status, not two: a single stale pill plus one sentence of reason.
    expect(screen.getAllByText("Not updating")).toHaveLength(1);
    expect(screen.getByText(/The last 2 tries to get new scores failed/)).toBeInTheDocument();
  });
});

describe("the age of cached ideas", () => {
  it("says how old the waiver ideas are once they stop being current", () => {
    render(<SeasonScreen view={view({ analysis_as_of_secs: NOW() - 420 })} />);
    expect(screen.getByText(/ideas from 7 minutes ago/)).toBeInTheDocument();
  });

  it("says nothing while the ideas are still fresh", () => {
    render(<SeasonScreen view={view({ analysis_as_of_secs: NOW() - 60 })} />);
    expect(screen.queryByText(/ideas from/)).not.toBeInTheDocument();
    // And nothing at all when the backend did not stamp the view.
    render(<SeasonScreen view={view()} />);
    expect(screen.queryByText(/ideas from/)).not.toBeInTheDocument();
  });
});
