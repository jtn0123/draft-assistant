// The Projections screen: the season model's forecast read as a forecast.
//
// The numbers exist elsewhere — the Season screen's standings carry the same
// projected points and playoff odds — but ordered by seed, next to a record,
// they read as history. Here the league is ordered by what it projects from
// here, so what is asserted is the ordering, the seed each row is moving
// against, and this week's price.

import { render, screen, within } from "@testing-library/react";
import { expect, it } from "vitest";
import type { SeasonView, StandingsRow } from "../season-types";
import type { DraftView, TeamProjection } from "../types";
import { draftFixture, seasonFixture } from "../test/appHarness";
import { ProjectionsScreen } from "./ProjectionsScreen";

function team(over: Partial<StandingsRow> & { roster_id: number }): StandingsRow {
  return {
    seed: 1,
    name: `Team ${over.roster_id}`,
    record: "1-1",
    wins: 1,
    losses: 1,
    ties: 0,
    points_for: 200,
    projected_points: 1500,
    playoff_odds: 0.5,
    is_mine: false,
    ...over,
  };
}

/** A league whose table order and seed order disagree, which is the point. */
function league(overrides: Partial<SeasonView> = {}): SeasonView {
  return seasonFixture({
    standings: [
      team({
        roster_id: 1,
        seed: 1,
        name: "Front runner",
        projected_points: 1500,
        playoff_odds: 0.7,
      }),
      team({
        roster_id: 2,
        seed: 3,
        name: "You",
        projected_points: 1700,
        playoff_odds: 0.91,
        is_mine: true,
        points_for: 210,
      }),
      team({ roster_id: 3, seed: 2, name: "Middling", projected_points: 1600, playoff_odds: 0.4 }),
    ],
    ...overrides,
  });
}

/** A draft with no projections on it, so the season half is what is asserted;
 *  the draft half has tests of its own below. */
function board(overrides: Partial<DraftView> = {}): DraftView {
  return { ...draftFixture(), draft_projections: [], ...overrides };
}

const rows = () =>
  [...document.querySelectorAll(".proj-body")].map((row) =>
    [...row.children].map((cell) => cell.textContent?.trim()),
  );

it("orders the league by what it projects, not by the seed it holds", () => {
  render(<ProjectionsScreen draft={board()} season={league()} />);

  expect(rows().map((cells) => cells[1])).toEqual(["You", "Middling", "Front runner"]);
  // Projected first, then what has actually been scored, then the odds.
  expect(rows()[0]).toEqual(["1", "You", "1-1", "210", "1700", "91%", "+2"]);
  // The front runner projects third from a first seed: two places to lose.
  expect(rows()[2]?.[6]).toBe("-2");
});

it("marks your own row and heads the screen with your two numbers", () => {
  const { container } = render(<ProjectionsScreen draft={board()} season={league()} />);

  const mine = container.querySelectorAll(".proj-body.is-mine");
  expect(mine).toHaveLength(1);
  expect(mine[0]).toHaveTextContent("You");
  // Rest-of-season points and playoff odds, headlined: the same 1700 the row
  // carries, and the header's own playoff number.
  const top = container.querySelectorAll(".proj-stat-value");
  expect([...top].map((stat) => stat.textContent)).toEqual(["1700", "88%"]);
});

it("prices this week and says what is sitting on the bench", () => {
  render(<ProjectionsScreen draft={board()} season={league()} />);

  const week = document.querySelector(".proj-week");
  if (week === null) throw new Error("the week panel is on screen");
  expect(within(week as HTMLElement).getByText("122.4")).toBeInTheDocument();
  expect(within(week as HTMLElement).getByText("108.9")).toBeInTheDocument();
  expect(within(week as HTMLElement).getByText("+13.5")).toBeInTheDocument();
  expect(within(week as HTMLElement).getByText("62% to win")).toBeInTheDocument();
  // 122.4 best against 118.1 as set: the difference is the reason to look.
  expect(screen.getByText(/4.3 points are on your bench/)).toBeInTheDocument();
});

it("says where a finished season landed rather than a flat 100%", () => {
  const view = league();
  view.header.playoff_status = "In the playoffs: seed 3";
  view.standings[1].playoff_status = "Missed the playoffs";
  render(<ProjectionsScreen draft={board()} season={view} />);

  expect(screen.getByText("In the playoffs: seed 3")).toBeInTheDocument();
  expect(screen.getByText("Missed the playoffs")).toBeInTheDocument();
});

it("leaves the week out when there is no opponent to price it against", () => {
  const view = league();
  view.header.opponent_name = null;
  render(<ProjectionsScreen draft={board()} season={view} />);

  expect(document.querySelector(".proj-week")).toBeNull();
  expect(document.querySelectorAll(".proj-body")).toHaveLength(3);
});

// What the draft itself says, which is the half of this screen that exists
// before a season does.
function drafted(over: Partial<TeamProjection> & { slot: number }): TeamProjection {
  return {
    name: `Slot ${over.slot}`,
    starters: 1500,
    bench: 300,
    holes: [],
    rank: 1,
    title_odds: 0.5,
    is_mine: false,
    ...over,
  };
}

it("leads with the draft: who drafted the most points, and what it is worth", () => {
  const view = board({
    draft_projections: [
      drafted({ slot: 4, name: "Sharks", starters: 1900, bench: 420, rank: 1, title_odds: 0.31 }),
      drafted({
        slot: 1,
        name: "You",
        starters: 1820,
        bench: 380,
        rank: 2,
        title_odds: 0.24,
        is_mine: true,
      }),
      drafted({
        slot: 7,
        name: "Half a team",
        starters: 900,
        bench: 0,
        rank: 3,
        title_odds: 0.01,
        holes: ["TE", "K", "DEF"],
      }),
    ],
  });
  render(<ProjectionsScreen draft={view} season={null} />);

  const table = [...document.querySelectorAll(".proj-draft-row.proj-body")].map((row) =>
    [...row.children].map((cell) => cell.textContent?.trim()),
  );
  expect(table.map((cells) => cells[1])).toEqual(["Sharks", "You", "Half a team"]);
  expect(table[1]).toEqual(["2", "You", "1820", "380", "24%", "·"]);
  // A roster still missing starters says which, rather than looking merely bad.
  expect(table[2]?.[5]).toBe("TE, K, DEF");
  // Your two numbers head the screen: what you drafted and what it wins.
  const top = [...document.querySelectorAll(".proj-stat-value")].map((s) => s.textContent);
  expect(top).toEqual(["1820 pts", "24%"]);
  expect(screen.getByText("2nd of 3 on projected starters")).toBeInTheDocument();
});

it("shows the draft alone until the season has anything to say", () => {
  const view = board({ draft_projections: [drafted({ slot: 1, is_mine: true })] });
  render(<ProjectionsScreen draft={view} season={null} />);

  expect(document.querySelectorAll(".proj-draft-row.proj-body")).toHaveLength(1);
  expect(screen.queryByText("From the season so far")).not.toBeInTheDocument();
  expect(document.querySelector(".proj-week")).toBeNull();
});

it("says so plainly when there is neither a draft order nor a season", () => {
  render(<ProjectionsScreen draft={board()} season={null} />);

  expect(screen.getByText(/Nothing to project yet/)).toBeInTheDocument();
});

// Ranks 11, 12 and 13 are the ones an ordinal rule gets wrong: "11st" reads
// as a bug in a sentence a user is meant to trust.
it("writes the teens as teens when the league is big enough to have them", () => {
  const rows = Array.from({ length: 13 }, (_, at) =>
    drafted({ slot: at + 1, starters: 2000 - at, rank: at + 1, is_mine: at === 10 }),
  );
  render(<ProjectionsScreen draft={board({ draft_projections: rows })} season={null} />);

  expect(screen.getByText("11th of 13 on projected starters")).toBeInTheDocument();
});

it("counts a roster with no seat of yours out of the headline", () => {
  const rows = [drafted({ slot: 2, rank: 1 }), drafted({ slot: 3, rank: 2 })];
  render(<ProjectionsScreen draft={board({ draft_projections: rows })} season={null} />);

  const top = [...document.querySelectorAll(".proj-stat-value")].map((stat) => stat.textContent);
  expect(top).toEqual(["-", "-"]);
  expect(screen.getByText("no seat in this draft")).toBeInTheDocument();
  // The table is still the league's, whether or not you are in it.
  expect(document.querySelectorAll(".proj-draft-row.proj-body")).toHaveLength(2);
});
