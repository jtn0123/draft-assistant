import { render, screen, within } from "@testing-library/react";
import { describe, expect, it } from "vitest";
import fixtureJson from "../../public/dev-fixture.json";
import type { DraftView, Starter, TeamProjection } from "../types";
import { SeasonScreen } from "./SeasonScreen";

function view(): DraftView {
  const v = structuredClone(fixtureJson) as unknown as DraftView;
  v.draft.status = "complete";
  return v;
}
const st = (slot: string, name: string, points: number): Starter => ({
  slot,
  player_id: name,
  name,
  position: slot,
  points,
  injury: null,
});
const team = (slot: number, name: string, season: number): TeamProjection => ({
  slot,
  display_name: name,
  full_strength: season,
  season,
  starters: [],
  week: 1,
  week_points: 0,
  week_starters: [],
});

function inSeason(): DraftView {
  const v = view();
  v.this_week = {
    week: 1,
    lineup: {
      set_points: 119.3,
      best_points: 125.5,
      changes: [
        { slot: "FLEX", out: st("RB", "Kenny Gainwell", 9.4), in_: st("WR", "Khalil Shakir", 11.5), gain: 2.1 },
        { slot: "DEF", out: null, in_: st("DEF", "New York Giants", 6.2), gain: 6.2 },
      ],
      empty_slots: ["DEF"], questionable: [],
    },
    matchup: {
      opponent_slot: 6,
      opponent_name: "MeatballMike09",
      my_points: 119.3,
      opponent_points: 125.4,
      margin: -6.1,
      win_probability: 0.42,
      my_starters: [st("QB", "Matthew Stafford", 20.7)],
      opponent_starters: [st("QB", "Kyler Murray", 18.1), st("DEF", "Texans", 7)],
    },
  };
  v.projected_standings = [team(9, "tbonsey", 2301), team(v.draft.my_slot!, "McSleeper26", 1845)];
  v.playoff_odds = [
    { slot: 9, display_name: "tbonsey", playoff_odds: 0.996, expected_wins: 11, expected_points: 2301, runs: 2000 },
    { slot: v.draft.my_slot!, display_name: "McSleeper26", playoff_odds: 0.117, expected_wins: 6, expected_points: 1845, runs: 2000 },
  ];
  return v;
}

describe("SeasonScreen", () => {
  it("says there is no week yet when the schedule is not out", () => {
    const v = view();
    v.this_week = null;
    render(<SeasonScreen view={v} />);
    expect(screen.getByText(/No week on the calendar yet/)).toBeInTheDocument();
    expect(screen.queryByRole("status")).not.toBeInTheDocument();
    // The league side still renders: the roster is always there.
    expect(screen.getByRole("heading", { level: 2, name: "My roster" })).toBeInTheDocument();
    // Nothing from the draft cockpit.
    expect(screen.queryByRole("heading", { name: "Tier alerts" })).not.toBeInTheDocument();
    expect(screen.queryByRole("heading", { name: "Recent picks" })).not.toBeInTheDocument();
  });

  it("puts the week in the banner: opponent, odds, projected place, playoffs", () => {
    render(<SeasonScreen view={inSeason()} />);
    const banner = screen.getByLabelText("Week 1 summary");
    expect(banner).toHaveTextContent("Week1");
    expect(banner).toHaveTextContent("vs MeatballMike09");
    expect(banner).toHaveTextContent("119.3 – 125.4 · 42% to win");
    expect(banner).toHaveTextContent("0–0");
    expect(banner).toHaveTextContent("projected 2nd of 2");
    expect(banner).toHaveTextContent("12%");
  });

  it("shouts once about a lineup that is not the best, and lays both lineups out", () => {
    render(<SeasonScreen view={inSeason()} />);
    expect(screen.getByRole("status")).toHaveTextContent(
      "Your lineup on Sleeper is not your best — DEF empty · 1 swap · +6.2 on the table",
    );
    expect(screen.getByRole("heading", { level: 2, name: "Lineup check" })).toBeInTheDocument();
    const table = screen.getByRole("table", { name: "Lineups side by side" });
    expect(within(table).getByText("Matthew Stafford")).toBeInTheDocument();
    expect(within(table).getByText("empty")).toHaveClass("empty");
  });

  it("uses the real record and place once games have been played", () => {
    const v = inSeason();
    v.season = {
      through_week: 3,
      standings: [
        { slot: 9, display_name: "tbonsey", wins: 3, losses: 0, ties: 0, points_for: 400, points_against: 300 },
        { slot: v.draft.my_slot!, display_name: "McSleeper26", wins: 2, losses: 1, ties: 0, points_for: 350, points_against: 340 },
      ],
      my_results: [],
      trends: [],
    };
    render(<SeasonScreen view={v} />);
    const banner = screen.getByLabelText("Week 1 summary");
    expect(banner).toHaveTextContent("2–1");
    expect(banner).toHaveTextContent("2nd of 2");
    expect(banner).not.toHaveTextContent("projected");
  });
});
