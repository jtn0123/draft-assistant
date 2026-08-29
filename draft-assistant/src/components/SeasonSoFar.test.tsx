import { render, screen } from "@testing-library/react";
import { describe, expect, it } from "vitest";
import fixtureJson from "../../public/dev-fixture.json";
import type { DraftView } from "../types";
import { SeasonSoFar } from "./SeasonSoFar";

function view(): DraftView {
  return structuredClone(fixtureJson) as unknown as DraftView;
}

describe("SeasonSoFar", () => {
  it("renders nothing before a week has been played", () => {
    const v = view();
    v.season = null;
    const { container } = render(<SeasonSoFar view={v} />);
    expect(container).toBeEmptyDOMElement();
  });

  it("shows my record and place, each result, and the trends", () => {
    const v = view();
    v.draft.my_slot = 2;
    v.season = {
      through_week: 2,
      standings: [
        { slot: 10, display_name: "tbonsey", wins: 2, losses: 0, ties: 0, points_for: 290.4, points_against: 200 },
        { slot: 2, display_name: "McSleeper26", wins: 1, losses: 1, ties: 0, points_for: 241.2, points_against: 230 },
        { slot: 6, display_name: "MeatballMike09", wins: 0, losses: 2, ties: 0, points_for: 200, points_against: 260 },
      ],
      my_results: [
        { week: 1, my_points: 125.5, opponent_slot: 6, opponent_name: "MeatballMike09", opponent_points: 118.2, won: true },
        { week: 2, my_points: 115.7, opponent_slot: 10, opponent_name: "tbonsey", opponent_points: 150.1, won: false },
      ],
      trends: [
        { player_id: "a", name: "Emeka Egbuka", position: "WR", games: 2, projected: 24, actual: 33, delta_per_game: 4.5 },
        { player_id: "b", name: "Sam LaPorta", position: "TE", games: 2, projected: 24, actual: 17, delta_per_game: -3.5 },
      ],
    };
    render(<SeasonSoFar view={v} />);
    expect(screen.getByRole("heading", { name: "Season through week 2" })).toBeInTheDocument();
    expect(screen.getByText(/1–1 · 2nd of 3 · 241 for/)).toBeInTheDocument();
    const rows = screen.getAllByRole("listitem");
    expect(rows[0]).toHaveClass("won");
    expect(rows[0]).toHaveTextContent("125.5 – 118.2");
    expect(rows[1]).toHaveClass("lost");
    expect(screen.getByText(/Beating projection: Emeka Egbuka \+4\.5\/g over 2/)).toBeInTheDocument();
    expect(screen.getByText(/Behind it: Sam LaPorta -3\.5\/g over 2/)).toBeInTheDocument();
  });
});
