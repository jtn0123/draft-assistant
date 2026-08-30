import { render, screen } from "@testing-library/react";
import userEvent from "@testing-library/user-event";
import { describe, expect, it } from "vitest";
import type { LineupCall, MatchupView, WaiverTarget } from "../season-types";
import { CallsToMake, LineupCompare, Waivers } from "./ThisWeek";

function call(slot: string, gain: number, why: string): LineupCall {
  return {
    slot,
    player_in: `In ${slot}`,
    player_in_id: `in-${slot}`,
    player_in_team: "BUF",
    player_out: `Out ${slot}`,
    player_out_id: `out-${slot}`,
    gain,
    why,
  };
}

describe("CallsToMake", () => {
  it("says so plainly when the lineup is already optimal", () => {
    render(<CallsToMake calls={[]} pointsOnTable={0} />);
    expect(screen.getByText("Your lineup is already optimal")).toBeInTheDocument();
  });

  it("keeps each reason collapsed until its row is opened", async () => {
    const user = userEvent.setup();
    render(<CallsToMake calls={[call("WR", 4, "because slot")]} pointsOnTable={4} />);

    expect(screen.queryByText("because slot")).not.toBeInTheDocument();
    await user.click(screen.getByRole("button", { name: /over/ }));
    expect(screen.getByText("because slot")).toBeInTheDocument();
  });

  it("opens every reason at once from the header toggle", async () => {
    const user = userEvent.setup();
    render(
      <CallsToMake
        calls={[call("WR", 4, "reason one"), call("TE", 3.7, "reason two")]}
        pointsOnTable={7.7}
      />,
    );

    expect(screen.getByText("2 calls to make — 7.7 points on the table")).toBeInTheDocument();
    await user.click(screen.getByRole("button", { name: "Show all reasons" }));
    expect(screen.getByText("reason one")).toBeInTheDocument();
    expect(screen.getByText("reason two")).toBeInTheDocument();
  });

  it("signs gains with a real minus rather than a hyphen", () => {
    render(<CallsToMake calls={[call("WR", 4, "why")]} pointsOnTable={4} />);
    expect(screen.getByText("+4.0")).toBeInTheDocument();
  });
});

const bestRow = {
  slot: "QB",
  my_player_id: "a",
  my_name: "Jalen Hurts",
  my_team: "PHI",
  my_points: 22.4,
  opp_player_id: "b",
  opp_name: "Baker Mayfield",
  opp_team: "TB",
  opp_points: 15.2,
  margin: 7.2,
};

const matchup: MatchupView = {
  my_name: "Trust the Process",
  opp_name: "punt_god",
  my_avatar: null,
  opp_avatar: null,
  my_projected: 122.4,
  opp_projected: 108.9,
  // What is actually set starts a worse QB, so Best and Set differ.
  set_projected: 117.4,
  set_rows: [{ ...bestRow, my_name: "Bryce Young", my_points: 17.4, margin: 2.2 }],
  rows: [
    {
      slot: "QB",
      my_player_id: "a",
      my_name: "Jalen Hurts",
      my_team: "PHI",
      my_points: 22.4,
      opp_player_id: "b",
      opp_name: "Baker Mayfield",
      opp_team: "TB",
      opp_points: 15.2,
      margin: 7.2,
    },
  ],
};

describe("LineupCompare", () => {
  it("explains a bye week instead of rendering an empty table", () => {
    render(<LineupCompare matchup={null} winOdds={0.5} />);
    expect(screen.getByText("No matchup this week — you're on a bye.")).toBeInTheDocument();
  });

  it("toggles between the lineup you should start and the one you have set", async () => {
    const user = userEvent.setup();
    render(<LineupCompare matchup={matchup} winOdds={0.62} />);
    expect(screen.getByText("5.0 sitting on your bench")).toBeInTheDocument();
    expect(screen.getByText("122.4")).toBeInTheDocument();
    expect(screen.getByText("Jalen Hurts")).toBeInTheDocument();

    await user.click(screen.getByRole("button", { name: "Set" }));
    expect(screen.getByText("117.4")).toBeInTheDocument();
    expect(screen.getByText("Bryce Young")).toBeInTheDocument();
    expect(screen.queryByText("Jalen Hurts")).not.toBeInTheDocument();
  });

  it("says so when the set lineup is already the best one", () => {
    render(
      <LineupCompare
        matchup={{ ...matchup, set_projected: matchup.my_projected, set_rows: matchup.rows }}
        winOdds={0.62}
      />,
    );
    expect(screen.getByText("your lineup is already your best")).toBeInTheDocument();
  });

  it("shows both sides, the margin and the win odds", () => {
    render(<LineupCompare matchup={matchup} winOdds={0.62} />);
    expect(screen.getByText("122.4")).toBeInTheDocument();
    expect(screen.getByText("108.9")).toBeInTheDocument();
    expect(screen.getByText("+13.5 · 62% to win")).toBeInTheDocument();
    expect(screen.getByText("Jalen Hurts")).toBeInTheDocument();
  });

  it("switches between the table and scoreboard views", async () => {
    const user = userEvent.setup();
    render(<LineupCompare matchup={matchup} winOdds={0.62} />);

    expect(screen.getByText("Your player")).toBeInTheDocument();
    await user.click(screen.getByRole("button", { name: "Scoreboard" }));
    expect(screen.queryByText("Your player")).not.toBeInTheDocument();
    expect(screen.getByText("Jalen Hurts")).toBeInTheDocument();
  });

  it("puts the gap between the two teams in the table too", () => {
    render(<LineupCompare matchup={matchup} winOdds={0.62} />);
    const row = document.querySelectorAll(".lineup-row")[1];
    const cells = [...row.children];
    // slot, my player, my proj, the gap, their proj, their player.
    expect(cells).toHaveLength(6);
    expect(cells[3].className).toContain("lean");
    expect(cells[3].textContent).toBe("+7.2");
    expect(screen.getByText("Gap")).toBeInTheDocument();
  });

  it("puts the slot's gap between the two teams, signed from my side", async () => {
    const user = userEvent.setup();
    const both: MatchupView = {
      ...matchup,
      rows: [
        matchup.rows[0],
        { ...matchup.rows[0], slot: "RB", my_points: 4.1, opp_points: 12.9, margin: -8.8 },
        { ...matchup.rows[0], slot: "TE", my_points: 9, opp_points: 9, margin: 0 },
      ],
    };
    render(<LineupCompare matchup={both} winOdds={0.62} />);
    await user.click(screen.getByRole("button", { name: "Scoreboard" }));

    const row = document.querySelector(".scoreboard-row");
    const cells = [...(row?.children ?? [])];
    // slot, my side, the gap, their side — the gap sits in the middle.
    expect(cells[2]?.className).toContain("lean");

    const leans = [...document.querySelectorAll(".lean")];
    expect(leans[0].className).toContain("is-mine");
    expect(leans[0].textContent).toBe("+7.2");
    expect(leans[1].className).toContain("is-theirs");
    expect(leans[1].textContent).toBe("−8.8");
    expect(leans[2].textContent).toBe("—");
  });

  it("shows a headshot on both sides of every scoreboard row", async () => {
    const user = userEvent.setup();
    render(<LineupCompare matchup={matchup} winOdds={0.62} />);
    await user.click(screen.getByRole("button", { name: "Scoreboard" }));
    const heads = document.querySelectorAll(
      ".scoreboard .headshot, .scoreboard .team-logo",
    );
    expect(heads).toHaveLength(2 * matchup.rows.length);
  });
});

function waiver(name: string, bid: number | null, rivals: number): WaiverTarget {
  return {
    player_id: name,
    name,
    position: "RB",
    team: "JAX",
    gain_points: 2.4,
    gain_fraction: 0.14,
    suggested_bid: bid,
    rivals,
  };
}

describe("Waivers", () => {
  it("reports the budget against the league total, and the competition", () => {
    render(<Waivers waivers={[waiver("Tuten", 12, 3)]} budgetLeft={38} budgetTotal={100} />);
    expect(screen.getByText("$38 of $100 left")).toBeInTheDocument();
    expect(screen.getByText("+14%")).toBeInTheDocument();
    expect(screen.getByText("$12 · 3 rivals")).toBeInTheDocument();
  });

  it("says nobody rather than '0 rivals'", () => {
    render(<Waivers waivers={[waiver("Kraft", 4, 0)]} budgetLeft={38} budgetTotal={null} />);
    expect(screen.getByText("$38 left")).toBeInTheDocument();
    expect(screen.getByText("$4 · nobody")).toBeInTheDocument();
  });

  it("handles a league with no FAAB budget", () => {
    render(<Waivers waivers={[waiver("Kraft", null, 1)]} budgetLeft={null} budgetTotal={null} />);
    expect(screen.getByText("no FAAB budget")).toBeInTheDocument();
    expect(screen.getByText("— · 1 rival")).toBeInTheDocument();
  });

  it("explains an empty list instead of showing a blank panel", () => {
    render(<Waivers waivers={[]} budgetLeft={38} budgetTotal={100} />);
    expect(
      screen.getByText(/No free agent would crack your starting lineup/),
    ).toBeInTheDocument();
  });
});
