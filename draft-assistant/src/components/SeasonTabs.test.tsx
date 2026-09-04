import { render, screen } from "@testing-library/react";
import userEvent from "@testing-library/user-event";
import { afterEach, describe, expect, it, vi } from "vitest";
import type { LastSeasonRow, RosterRow, StandingsRow } from "../season-types";
import { LastSeason, Standings, TeamRoster } from "./SeasonTabs";

function row(overrides: Partial<RosterRow>): RosterRow {
  return {
    player_id: "1",
    name: "Josh Downs",
    position: "WR",
    team: "IND",
    role: "Bench",
    points: 0,
    projected: 12.8,
    ...overrides,
  };
}

// Fake timers are installed by the one test that needs them; this makes sure
// they cannot leak into the rest of this file or any other.
afterEach(() => {
  vi.useRealTimers();
});

describe("TeamRoster empty state", () => {
  it("points at the username setting when there is no roster", () => {
    render(<TeamRoster rows={[]} />);
    expect(screen.getByText(/Set your Sleeper username/)).toBeInTheDocument();
  });
});

describe("TeamRoster", () => {
  it("shows this week's projection beside the season total", () => {
    render(
      <TeamRoster
        rows={[
          row({ player_id: "1", role: "Start", projected: 23.4, points: 0 }),
          row({
            player_id: "2",
            name: "Tony Pollard",
            role: "Bench",
            projected: 9.1,
            points: 41.5,
          }),
        ]}
      />,
    );
    expect(screen.getByText("23.4")).toBeInTheDocument();
    expect(screen.getByText("41.5")).toBeInTheDocument();
    expect(screen.getByText("Wk")).toBeInTheDocument();
    expect(screen.getByText("Season")).toBeInTheDocument();
  });

  it("em-dashes a column that has nothing in it yet", () => {
    // Week one, pre-kickoff: nobody has scored, so the Season column is a
    // stack of "0.0" that reads as fifteen men measured at zero.
    const { container } = render(
      <TeamRoster
        rows={[
          row({ player_id: "1", role: "Start", projected: 23.4, points: 0 }),
          row({ player_id: "2", name: "Tony Pollard", role: "Bench", projected: 9.1, points: 0 }),
        ]}
      />,
    );
    const seasons = [...container.querySelectorAll(".team-season")].map((c) => c.textContent);
    expect(seasons).toEqual(["—", "—"]);
    // The projections are real, so that column still prints.
    expect(screen.getByText("23.4")).toBeInTheDocument();
    expect(screen.getByText(/A dash means that column has nothing in it yet/)).toBeInTheDocument();
  });

  it("keeps a zero that sits beside a real number", () => {
    const { container } = render(
      <TeamRoster
        rows={[
          row({ player_id: "1", role: "Start", projected: 0, points: 0 }),
          row({
            player_id: "2",
            name: "Tony Pollard",
            role: "Bench",
            projected: 9.1,
            points: 41.5,
          }),
        ]}
      />,
    );
    // One team-mate having scored makes every other zero a measurement.
    const seasons = [...container.querySelectorAll(".team-season")].map((c) => c.textContent);
    expect(seasons).toEqual(["0.0", "41.5"]);
  });

  it("writes Bye instead of a projection on a bye week", () => {
    render(<TeamRoster rows={[row({ role: "Bye", projected: 0, points: 30 })]} />);
    // Once as the role, once in place of the projection.
    expect(screen.getAllByText("Bye")).toHaveLength(2);
    expect(screen.getByText("30.0")).toBeInTheDocument();
  });
});
function standing(overrides: Partial<StandingsRow>): StandingsRow {
  return {
    roster_id: 1,
    seed: 1,
    name: "AllDay21",
    record: "0–0",
    wins: 0,
    losses: 0,
    ties: 0,
    points_for: 0,
    projected_points: 100,
    playoff_odds: 0.5,
    is_mine: false,
    ...overrides,
  };
}

describe("Standings", () => {
  const rows = [
    standing({
      roster_id: 1,
      seed: 1,
      name: "AllDay21",
      wins: 2,
      projected_points: 130,
      playoff_odds: 0.9,
    }),
    standing({
      roster_id: 2,
      seed: 2,
      name: "Witzy",
      wins: 1,
      projected_points: 110,
      playoff_odds: 0.4,
      is_mine: true,
    }),
    standing({
      roster_id: 3,
      seed: 3,
      name: "Bears",
      wins: 0,
      losses: 2,
      projected_points: 90,
      playoff_odds: 0.1,
    }),
  ];

  const order = (container: HTMLElement) =>
    [...container.querySelectorAll(".standings-row:not(.standings-head) .team-cell .ellipsis")].map(
      (cell) => cell.textContent,
    );

  it("orders by seed, highlights my row, and formats odds as a percentage", () => {
    const { container } = render(<Standings rows={rows} />);
    expect(order(container)).toEqual(["AllDay21", "Witzy", "Bears"]);
    expect(container.querySelector(".standings-row.is-mine")).toHaveTextContent("Witzy");
    expect(screen.getByText("90%")).toBeInTheDocument();
  });

  it("re-sorts on a column header and flips direction on a second click", async () => {
    const user = userEvent.setup();
    const { container } = render(<Standings rows={rows} />);
    const proj = screen.getByRole("button", { name: /^Proj,/i });
    expect(proj).toHaveAccessibleName("Proj, not sorted");
    await user.click(proj);
    expect(order(container)[0]).toBe("AllDay21");
    expect(proj).toHaveAccessibleName("Proj, sorted descending");
    await user.click(proj);
    expect(order(container)[0]).toBe("Bears");
    expect(proj).toHaveAccessibleName("Proj, sorted ascending");
  });

  it("sorts by team name alphabetically when that header is clicked", async () => {
    const user = userEvent.setup();
    const { container } = render(<Standings rows={rows} />);
    await user.click(screen.getByRole("button", { name: /^Team,/i }));
    expect(order(container)).toEqual(["AllDay21", "Bears", "Witzy"]);
  });

  it("explains itself when the league has no rosters yet", () => {
    render(<Standings rows={[]} />);
    expect(screen.getByText(/once the league has rosters/)).toBeInTheDocument();
  });
});
function finish(overrides: Partial<LastSeasonRow>): LastSeasonRow {
  return {
    place: 1,
    name: "AllDay21",
    record: "11–3",
    points: 1800,
    tag: null,
    is_mine: false,
    ...overrides,
  };
}

describe("LastSeason", () => {
  it("names last year's champ and where I finished, with ordinal suffixes", () => {
    render(
      <LastSeason
        season="2026"
        rows={[
          finish({ place: 1, tag: "Champ" }),
          finish({ place: 2, name: "Witzy", tag: "Most pts" }),
          finish({ place: 3, name: "Bears" }),
          finish({ place: 11, name: "Me", is_mine: true }),
        ]}
      />,
    );
    expect(screen.getByText("2025 final")).toBeInTheDocument();
    expect(screen.getByText("you finished 11th")).toBeInTheDocument();
    const champ = screen.getByText("Champ");
    expect(champ.className).toContain("is-champ");
    expect(screen.getByText("Most pts").className).toContain("is-most");
    const mine = screen.getByText("Me").closest(".last-row");
    expect(mine?.className).toContain("is-mine");
  });

  it("handles the ordinals that break the simple suffix rule", () => {
    render(
      <LastSeason
        season="2026"
        rows={[
          finish({ place: 1, name: "First", is_mine: true }),
          finish({ place: 2, name: "Second" }),
          finish({ place: 3, name: "Third" }),
          finish({ place: 12, name: "Twelfth" }),
          finish({ place: 13, name: "Thirteenth" }),
        ]}
      />,
    );
    expect(screen.getByText("you finished 1st")).toBeInTheDocument();
  });

  it("falls back gracefully when the league has no linked history", () => {
    render(<LastSeason season="2026" rows={[]} />);
    expect(screen.getByText(/No previous season is linked/)).toBeInTheDocument();
  });

  it("titles the section 'Last season' when the season is not numeric", () => {
    render(<LastSeason season="preseason" rows={[finish({})]} />);
    expect(screen.getByText("Last season")).toBeInTheDocument();
  });
});

describe("Standings once the bracket is cut", () => {
  /** From week 15 the simulation has nothing left to run and hands back a
   *  flat 100%/0%, which the table printed as though it were a forecast. */
  it("prints the bracket state in place of the percentage", () => {
    render(
      <Standings
        rows={[
          standing({ roster_id: 1, playoff_odds: 1, playoff_status: "In the playoffs — seed 1" }),
          standing({ roster_id: 2, playoff_odds: 0, playoff_status: "Missed the playoffs" }),
        ]}
        avatars={{}}
      />,
    );
    expect(screen.getByText("In the playoffs — seed 1")).toBeInTheDocument();
    expect(screen.getByText("Missed the playoffs")).toBeInTheDocument();
    expect(screen.queryByText("100%")).not.toBeInTheDocument();
  });
});
