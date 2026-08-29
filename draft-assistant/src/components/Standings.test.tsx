import { render, screen } from "@testing-library/react";
import userEvent from "@testing-library/user-event";
import { describe, expect, it } from "vitest";
import fixtureJson from "../../public/dev-fixture.json";
import type { DraftView, TeamProjection } from "../types";
import { Standings } from "./Standings";

function view(): DraftView {
  return structuredClone(fixtureJson) as unknown as DraftView;
}

function team(slot: number, name: string, season: number): TeamProjection {
  return {
    slot,
    display_name: name,
    full_strength: season + 40,
    season,
    starters: [
      { slot: "QB", player_id: "q", name: "Some QB", position: "QB", points: 300, injury: null },
    ],
    week: 1,
    week_points: season / 17,
    week_starters: [
      { slot: "QB", player_id: "q", name: "Some QB", position: "QB", points: 18.2, injury: null },
    ],
  };
}

describe("Standings", () => {
  it("ranks teams by bye-adjusted season points and marks mine", () => {
    const v = view();
    v.draft.my_slot = 2;
    v.projected_standings = [team(5, "ChrisWitz", 1901.4), team(2, "McSleeper26", 1850.2), team(9, "Clemmy20", 1700)];
    v.playoff_odds = [
      { slot: 5, display_name: "ChrisWitz", playoff_odds: 0.81, expected_wins: 16.2, expected_points: 1900, runs: 2000 },
      { slot: 2, display_name: "McSleeper26", playoff_odds: 0.44, expected_wins: 13.9, expected_points: 1850, runs: 2000 },
    ];
    render(<Standings view={v} />);
    // The header row is decorative; the teams follow it.
    const rows = screen.getAllByRole("listitem").filter((li) => !li.classList.contains("standings-head"));
    expect(rows).toHaveLength(3);
    expect(rows[0]).toHaveTextContent("1");
    expect(rows[0]).toHaveTextContent("ChrisWitz");
    expect(rows[0]).toHaveTextContent("1901");
    expect(rows[1]).toHaveClass("mine");
    expect(rows[1]).toHaveTextContent("YOU");
    // The gap to the leader, so "how far back am I" is read, not computed.
    expect(rows[1]).toHaveTextContent("−51");
    expect(rows[0].getAttribute("title")).toContain("QB Some QB");
    // Week 1 from the week's own rows, one decimal — a weekly number is small.
    expect(rows[0]).toHaveTextContent("111.8");
    expect(rows[0].getAttribute("title")).toContain("Week 1: 111.8");
    // Playoff odds from the simulation, blank for a team it has nothing on.
    expect(rows[0]).toHaveTextContent("81%");
    expect(rows[1]).toHaveTextContent("44%");
    expect(rows[2].querySelector(".standings-odds")).toHaveTextContent("");
  });

  it("opens a team's lineup on a tap, not only on hover", async () => {
    const user = userEvent.setup();
    const v = view();
    v.draft.my_slot = 2;
    v.projected_standings = [team(5, "ChrisWitz", 1901.4), team(2, "McSleeper26", 1850.2)];
    render(<Standings view={v} />);
    expect(screen.queryByLabelText("ChrisWitz lineup")).toBeNull();

    await user.click(screen.getByRole("button", { name: "Show ChrisWitz lineup" }));
    const panel = screen.getByLabelText("ChrisWitz lineup");
    expect(panel).toHaveTextContent("1941 at full strength");
    expect(panel).toHaveTextContent("QBSome QB300");
    expect(panel).toHaveTextContent("Week 1 · 111.8");
    expect(panel).toHaveTextContent("18.2");
    expect(screen.getByRole("button", { name: "Hide ChrisWitz lineup" })).toHaveAttribute("aria-expanded", "true");

    // One open at a time; my own row is named as such.
    await user.click(screen.getByRole("button", { name: "Show YOU lineup" }));
    expect(screen.queryByLabelText("ChrisWitz lineup")).toBeNull();
    expect(screen.getByLabelText("YOU lineup")).toBeInTheDocument();
  });

  it("leaves the week column blank when no team has rows for it", () => {
    const v = view();
    v.projected_standings = [{ ...team(5, "ChrisWitz", 1901.4), week_points: 0, week_starters: [] }];
    render(<Standings view={v} />);
    expect(screen.queryByText(/Wk 1/)).toBeNull();
  });

  it("renders nothing before there are any teams", () => {
    const v = view();
    v.projected_standings = [];
    const { container } = render(<Standings view={v} />);
    expect(container).toBeEmptyDOMElement();
  });
});
