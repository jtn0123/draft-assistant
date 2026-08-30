import { render, screen } from "@testing-library/react";
import { describe, expect, it } from "vitest";
import type { RosterRow } from "../season-types";
import { LeagueTab, TeamRoster } from "./SeasonTabs";

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

describe("TeamRoster", () => {
  it("shows this week's projection beside the season total", () => {
    render(
      <TeamRoster
        rows={[
          row({ player_id: "1", role: "Start", projected: 23.4, points: 0 }),
          row({ player_id: "2", name: "Tony Pollard", role: "Bench", projected: 9.1, points: 41.5 }),
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
