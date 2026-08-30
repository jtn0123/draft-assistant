import { render, screen } from "@testing-library/react";
import { describe, expect, it } from "vitest";
import type { LiveGame, LiveSection } from "../season-types";
import { GamesTab } from "./GamesTab";

function game(): LiveGame {
  return {
    game_id: "g1",
    away: "SF",
    home: "LAR",
    away_score: null,
    home_score: null,
    state: "pre",
    status: "",
    kickoff_ms: Date.parse("2026-09-13T17:00:00Z"),
    flag: null,
    channel: "Netflix",
    chips: [
      { player_id: "a", name: "McCaffrey", slot: "RB", team: "SF", points: 0, is_mine: true, state: "pre" },
      { player_id: "b", name: "Nacua", slot: "WR", team: "LAR", points: 0, is_mine: false, state: "pre" },
    ],
  };
}

function live(byeTeams: string[]): LiveSection {
  const g = game();
  return {
    games: [g],
    windows: [{ kickoff_ms: g.kickoff_ms, my_starters: 1, games: [g] }],
    totals: { my_playing: 0, my_pre: 1, my_done: 0, my_live_points: 0, opp_live_points: 0 },
    next_kickoff_ms: g.kickoff_ms,
    bye_teams: byeTeams,
  };
}

describe("GamesTab", () => {
  it("says which network is showing each game, until it is over", () => {
    render(<GamesTab live={live([])} myProjected={120} oppProjected={110} opponentName="punt_god" />);
    // Once in the window list, once on the this-week row.
    expect(screen.getAllByText("Netflix").length).toBeGreaterThan(0);
  });

  it("drops the network once the game is final", () => {
    const section = live([]);
    for (const g of [...section.games, ...section.windows[0].games]) g.state = "final";
    render(<GamesTab live={section} myProjected={120} oppProjected={110} opponentName="punt_god" />);
    expect(screen.queryByText("Netflix")).not.toBeInTheDocument();
  });

  it("names the opponent on the roster line and lists the byes in the footer", () => {
    render(
      <GamesTab live={live(["DEN", "LAC"])} myProjected={120} oppProjected={110} opponentName="punt_god" />,
    );
    expect(screen.getByText(/You: RB McCaffrey.*punt_god: WR Nacua/)).toBeInTheDocument();
    expect(
      screen.getByText(
        "Byes this week: DEN, LAC. Live scoring updates every 30s while a game is in progress.",
      ),
    ).toBeInTheDocument();
  });

  it("drops the byes sentence when no schedule has loaded", () => {
    render(<GamesTab live={live([])} myProjected={120} oppProjected={110} opponentName={null} />);
    expect(screen.getByText(/You: RB McCaffrey.*Them: WR Nacua/)).toBeInTheDocument();
    expect(
      screen.getByText("Live scoring updates every 30s while a game is in progress."),
    ).toBeInTheDocument();
    expect(screen.queryByText(/Byes this week/)).not.toBeInTheDocument();
  });
});
