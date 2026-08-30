import { render, screen, within } from "@testing-library/react";
import userEvent from "@testing-library/user-event";
import { describe, expect, it } from "vitest";
import type { LastSeasonRow, RosterRow, StandingsRow, TradeIdea } from "../season-types";
import { LastSeason, LeagueTab, Standings, TeamRoster } from "./SeasonTabs";

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

describe("LeagueTab", () => {
  it("dates every activity row and lists completed trades with both sides", () => {
    render(
      <LeagueTab
        trades={[]}
        recentTrades={[
          {
            transaction_id: "t1",
            at: Date.parse("2026-08-25T14:30:00Z"),
            involves_me: true,
            pending: false,
            sides: [
              { roster_id: 11, team: "Stompthatass", gets: ["Jaxon Smith-Njigba"] },
              { roster_id: 13, team: "Da Little Bears", gets: [] },
            ],
          },
        ]}
        activity={[
          {
            kind: "Add",
            text: "ocrevo added Adonai Mitchell",
            created: Date.parse("2026-08-30T17:57:00Z"),
            roster_id: 12,
            player_ids: ["11624"],
          },
        ]}
      />,
    );
    expect(screen.getByText("1 completed")).toBeInTheDocument();
    expect(screen.getByText(/gets draft picks/)).toBeInTheDocument();
    expect(screen.getByText("Aug 25, 10:30 AM")).toBeInTheDocument();
    expect(screen.getByText("Aug 30, 1:57 PM")).toBeInTheDocument();
    expect(screen.queryByText("In review")).not.toBeInTheDocument();
  });

  it("gives every activity row a tinted kind chip and the faces in the move", () => {
    const { container } = render(
      <LeagueTab
        trades={[]}
        recentTrades={[]}
        activity={[
          {
            kind: "Trade",
            text: "AllDay21 gets Tyler Warren",
            created: Date.parse("2026-08-30T17:57:00Z"),
            roster_id: 3,
            player_ids: ["11624", "9226"],
          },
        ]}
      />,
    );
    expect(container.querySelector(".activity-kind.is-trade")).not.toBeNull();
    expect(container.querySelectorAll(".activity-faces .avatar")).toHaveLength(2);
  });

  it("marks a trade the league has not processed yet", () => {
    render(
      <LeagueTab
        trades={[]}
        recentTrades={[
          {
            transaction_id: "t2",
            at: Date.parse("2026-08-29T14:30:00Z"),
            involves_me: false,
            pending: true,
            sides: [
              { roster_id: 11, team: "Stompthatass", gets: ["Bijan Robinson"] },
              { roster_id: 13, team: "Meatball", gets: ["Puka Nacua"] },
            ],
          },
        ]}
        activity={[]}
      />,
    );
    expect(screen.getByText("In review")).toBeInTheDocument();
    expect(screen.getByText("1 in review · 0 completed")).toBeInTheDocument();
  });
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

describe("LeagueTab trade ideas", () => {
  const idea: TradeIdea = {
    roster_id: 4,
    partner: "Meatball",
    get_id: "4881",
    get_name: "Lamar Jackson",
    get_position: "QB",
    give_id: "6786",
    give_name: "Jerry Jeudy",
    give_position: "WR",
    my_edge: 2.4,
    their_edge: 1.1,
    note: "Meatball needs a QB",
  };

  it("shows the swap, my weekly edge, and the partner note", () => {
    render(<LeagueTab trades={[idea]} recentTrades={[]} activity={[]} />);
    expect(screen.getByText("Lamar Jackson")).toBeInTheDocument();
    expect(screen.getByText("Jerry Jeudy")).toBeInTheDocument();
    expect(screen.getByText("+2.4 / wk")).toBeInTheDocument();
    expect(screen.getByText(/Meatball needs a QB/)).toBeInTheDocument();
  });

  it("says so when no swap helps, and when nothing has moved", () => {
    render(<LeagueTab trades={[]} recentTrades={[]} activity={[]} />);
    expect(screen.getByText(/No swap would improve both rosters/)).toBeInTheDocument();
    expect(screen.getByText(/Nothing has moved lately/)).toBeInTheDocument();
    expect(screen.getByText(/none this week or last/)).toBeInTheDocument();
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

describe("LeagueTab manager avatars", () => {
  it("puts the manager's picture on rows that name a manager", () => {
    const { container } = render(
      <LeagueTab
        trades={[]}
        recentTrades={[]}
        activity={[
          {
            kind: "Lineup",
            text: "Witzy benched Josh Downs",
            created: Date.parse("2026-08-30T12:00:00Z"),
            roster_id: 2,
            player_ids: [],
          },
          {
            kind: "Add",
            text: "Waivers cleared",
            created: Date.parse("2026-08-30T11:00:00Z"),
            roster_id: null,
            player_ids: [],
          },
        ]}
        avatars={{ "2": "abc123" }}
      />,
    );
    const rows = container.querySelectorAll(".activity-row");
    expect(
      within(rows[0] as HTMLElement).getByText("Witzy benched Josh Downs"),
    ).toBeInTheDocument();
    expect((rows[0] as HTMLElement).querySelector(".team-avatar")).not.toBeNull();
    expect((rows[1] as HTMLElement).querySelector(".team-avatar")).toBeNull();
  });
});
