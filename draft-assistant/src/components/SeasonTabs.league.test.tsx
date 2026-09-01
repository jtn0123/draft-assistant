import { render, screen, within } from "@testing-library/react";
import { afterEach, describe, expect, it, vi } from "vitest";
import type { TradeIdea } from "../season-types";
import { LeagueTab } from "./SeasonTabs";

// Fake timers are installed by the one test that needs them; this makes sure
// they cannot leak into the rest of this file or any other.
afterEach(() => {
  vi.useRealTimers();
});

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
            players: [{ id: "11624", name: "Adonai Mitchell", team: "IND" }],
          },
        ]}
      />,
    );
    expect(screen.getByText("1 completed")).toBeInTheDocument();
    // Nothing on the wire named the picks, so the vague line is all there is.
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
            players: [
              { id: "11624", name: "Adonai Mitchell", team: "IND" },
              { id: "9226", name: "Tyler Warren", team: "IND" },
            ],
          },
        ]}
      />,
    );
    expect(container.querySelector(".activity-kind.is-trade")).not.toBeNull();
    expect(container.querySelectorAll(".activity-faces .avatar")).toHaveLength(2);
  });

  it("falls back to a team mark, not an empty circle, when a face has no photo", () => {
    // The feed used to carry bare ids, so a player Sleeper has no photo of
    // (or whose photo failed to load once) left a blank hole here while the
    // same player showed his team logo everywhere else in the app.
    const { container } = render(
      <LeagueTab
        trades={[]}
        recentTrades={[]}
        activity={[
          {
            kind: "Add",
            text: "ocrevo added Mike Evans",
            created: Date.parse("2026-08-30T17:57:00Z"),
            roster_id: 12,
            players: [{ id: "2216", name: "Mike Evans", team: "TB" }],
          },
        ]}
      />,
    );
    const face = container.querySelector(".activity-faces .avatar");
    expect(face).toHaveClass("avatar-logo");
    expect(face).not.toHaveClass("avatar-blank");
    expect(face).toHaveAttribute("src", expect.stringContaining("/tb.png"));
    // ...and the zoom is captioned with his name rather than the word "player".
    expect(
      screen.getByRole("button", { name: "Show a larger picture of Mike Evans" }),
    ).toBeInTheDocument();
  });

  it("gives a trade idea's players their team mark when they have no photo", () => {
    const { container } = render(
      <LeagueTab
        trades={[
          {
            roster_id: 4,
            partner: "Meatball",
            get_id: "2216",
            get_name: "Mike Evans",
            get_position: "WR",
            get_team: "TB",
            give_id: "6786",
            give_name: "Jerry Jeudy",
            give_position: "WR",
            give_team: "DEN",
            my_edge: 2.4,
            their_edge: 1.1,
            note: "Meatball needs a WR",
          },
        ]}
        recentTrades={[]}
        activity={[]}
      />,
    );
    const marks = container.querySelectorAll(".trade-players .avatar");
    expect(marks).toHaveLength(2);
    expect(marks[0]).toHaveClass("avatar-logo");
    expect(marks[1]).toHaveClass("avatar-logo");
  });

  it("names the picks a side came away with", () => {
    render(
      <LeagueTab
        trades={[]}
        recentTrades={[
          {
            transaction_id: "t3",
            at: Date.parse("2026-08-25T14:30:00Z"),
            involves_me: false,
            pending: false,
            sides: [
              { roster_id: 11, team: "Stompthatass", gets: ["Bijan Robinson"] },
              { roster_id: 13, team: "Meatball", gets: ["2026 1st", "2027 3rd"] },
            ],
          },
        ]}
        activity={[]}
      />,
    );
    expect(screen.getByText(/gets 2026 1st, 2027 3rd/)).toBeInTheDocument();
    expect(screen.queryByText(/gets draft picks/)).not.toBeInTheDocument();
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

  // Grade item D8. Staleness is measured against the wall clock, so the clock
  // is stopped: the interesting assertion is the threshold itself, and a live
  // clock can only ever land near it.
  it("admits how old the trade ideas are once they stop being current", () => {
    vi.useFakeTimers();
    vi.setSystemTime(Date.parse("2026-09-13T17:00:00Z"));
    const nowSecs = Math.floor(Date.now() / 1000);
    const { rerender } = render(
      <LeagueTab trades={[]} recentTrades={[]} activity={[]} analysisAsOfSecs={nowSecs - 60} />,
    );
    // A minute old is still "now" as far as a reader is concerned.
    expect(screen.getByText("by roster fit")).toBeInTheDocument();
    expect(screen.queryByText(/ideas from/)).not.toBeInTheDocument();

    // One second short of the two-minute threshold: still current.
    rerender(
      <LeagueTab trades={[]} recentTrades={[]} activity={[]} analysisAsOfSecs={nowSecs - 119} />,
    );
    expect(screen.queryByText(/ideas from/)).not.toBeInTheDocument();

    // The threshold exactly — the first moment the note is owed to the reader.
    rerender(
      <LeagueTab trades={[]} recentTrades={[]} activity={[]} analysisAsOfSecs={nowSecs - 120} />,
    );
    expect(screen.getByText("ideas from 2 minutes ago")).toBeInTheDocument();

    rerender(
      <LeagueTab trades={[]} recentTrades={[]} activity={[]} analysisAsOfSecs={nowSecs - 420} />,
    );
    expect(screen.getByText("ideas from 7 minutes ago")).toBeInTheDocument();
    expect(screen.queryByText("by roster fit")).not.toBeInTheDocument();
  });
});

describe("LeagueTab trade ideas", () => {
  const idea: TradeIdea = {
    roster_id: 4,
    partner: "Meatball",
    get_id: "4881",
    get_name: "Lamar Jackson",
    get_position: "QB",
    get_team: "BAL",
    give_id: "6786",
    give_name: "Jerry Jeudy",
    give_position: "WR",
    give_team: "DEN",
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
            players: [],
          },
          {
            kind: "Add",
            text: "Waivers cleared",
            created: Date.parse("2026-08-30T11:00:00Z"),
            roster_id: null,
            players: [],
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
