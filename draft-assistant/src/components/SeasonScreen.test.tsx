import { render, screen } from "@testing-library/react";
import { describe, expect, it } from "vitest";
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

  it("falls back to the overall stamp for a view with no per-source detail", () => {
    render(<SeasonScreen view={view({ data_health: { fetched_at: NOW() - 4, warnings: [] } })} />);
    const badge = screen.getByText("Live · 4s ago");
    expect(badge).toHaveClass("pill-live");
    expect(badge).not.toHaveAttribute("title");
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
