import { render, screen, within } from "@testing-library/react";
import { describe, expect, it } from "vitest";
import type { MatchupPreview, Starter } from "../types";
import { MatchupTable } from "./MatchupTable";

const st = (slot: string, name: string, points: number, injury: string | null = null): Starter => ({
  slot,
  player_id: name,
  name,
  position: slot,
  points,
  injury,
});

function matchup(mine: Starter[], theirs: Starter[]): MatchupPreview {
  return {
    opponent_slot: 6,
    opponent_name: "MeatballMike09",
    my_points: mine.reduce((a, s) => a + s.points, 0),
    opponent_points: theirs.reduce((a, s) => a + s.points, 0),
    margin: 0,
    win_probability: 0.42,
    my_starters: mine,
    opponent_starters: theirs,
  };
}

describe("MatchupTable", () => {
  it("pairs lineups slot by slot, marks the edge, and shows a missing slot as empty", () => {
    render(
      <MatchupTable
        matchup={matchup(
          [st("QB", "Matthew Stafford", 20.7), st("RB", "Bijan Robinson", 19), st("RB", "TreVeyon Henderson", 11)],
          [st("QB", "Kyler Murray", 18.1), st("RB", "Josh Jacobs", 15), st("RB", "Kenneth Walker", 12), st("DEF", "Texans", 7)],
        )}
      />,
    );
    const rows = within(screen.getByRole("table", { name: "Lineups side by side" })).getAllByRole("row");
    // header, QB, RB, RB, DEF, total
    expect(rows).toHaveLength(6);
    expect(rows[0]).toHaveTextContent("MeatballMike09");
    expect(rows[1]).toHaveTextContent("QBMatthew Stafford20.7Kyler Murray18.1");
    expect(within(rows[1]).getByText("Matthew Stafford")).toHaveClass("edge");
    expect(within(rows[1]).getByText("Kyler Murray")).not.toHaveClass("edge");
    expect(within(rows[3]).getByText("Kenneth Walker")).toHaveClass("edge");
    expect(rows[4]).toHaveTextContent("DEF");
    const empty = within(rows[4]).getByText("empty");
    expect(empty).toHaveClass("empty");
    expect(rows[4]).toHaveTextContent("Texans7.0");
    expect(rows[5]).toHaveTextContent("Total50.752.1");
  });

  it("names an injury next to the player", () => {
    render(
      <MatchupTable
        matchup={matchup([st("WR", "Tee Higgins", 0, "Out")], [st("WR", "Nico Collins", 14.2)])}
      />,
    );
    expect(screen.getByText("Tee Higgins · Out")).toBeInTheDocument();
  });
});
