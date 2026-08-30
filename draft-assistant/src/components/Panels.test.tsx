import { render, screen } from "@testing-library/react";
import { describe, expect, it } from "vitest";
import fixtureJson from "../../public/dev-fixture.json";
import type { DraftView } from "../types";
import { SidePanel } from "./Panels";

function fixture(): DraftView {
  return structuredClone(fixtureJson) as unknown as DraftView;
}

describe("SidePanel", () => {
  it("names the at-risk deadline as the pick survival was judged against", () => {
    const view = fixture();
    view.draft.teams = 14;
    view.draft.is_my_pick = true;
    // Picks 27 and 30 are mine; survival is measured at 30, not the one I'm
    // making right now, and is shown in round.pick form.
    view.draft.my_next_picks = [27, 30, 55];

    render(<SidePanel view={view} />);
    expect(screen.getByText("Won't last to 3.02")).toBeInTheDocument();
    expect(screen.queryByText("Won't last to 2.13")).not.toBeInTheDocument();
  });

  it("uses the upcoming pick when it is not my turn", () => {
    const view = fixture();
    view.draft.teams = 14;
    view.draft.is_my_pick = false;
    view.draft.my_next_picks = [30, 55];

    render(<SidePanel view={view} />);
    expect(screen.getByText("Won't last to 3.02")).toBeInTheDocument();
  });

  it("falls back to a bare heading once I have no picks left", () => {
    const view = fixture();
    view.draft.is_my_pick = false;
    view.draft.my_next_picks = [];

    render(<SidePanel view={view} />);
    expect(screen.getByText("Won't last")).toBeInTheDocument();
  });

  it("flags a thin tier as urgent and leaves a deep one plain", () => {
    const view = fixture();
    view.tier_alerts = [
      { position: "TE", tier: 3, players_left: 1 },
      { position: "WR", tier: 4, players_left: 40 },
    ];

    render(<SidePanel view={view} />);
    expect(screen.getByText("1 left")).toHaveClass("is-urgent");
    expect(screen.getByText("25+ left")).not.toHaveClass("is-urgent");
  });

  it("labels recent picks as round.pick with the player's team mark", () => {
    const view = fixture();
    view.draft.teams = 14;
    view.recent_picks = [
      {
        pick_no: 31,
        round: 3,
        slot: 3,
        slot_name: "Bench Warmers",
        player_id: "cb",
        name: "Chase Brown",
        position: "RB",
        team: "CIN",
      },
    ];

    const { container } = render(<SidePanel view={view} />);
    expect(screen.getByText("3.03")).toBeInTheDocument();
    expect(screen.queryByText("31")).not.toBeInTheDocument();
    expect(container.querySelector('img[src$="/cin.png"]')).not.toBeNull();
  });

  it("says how heavy a position run is", () => {
    const view = fixture();
    view.position_run = { position: "RB", count: 4, window: 6 };

    render(<SidePanel view={view} />);
    expect(screen.getByText("RB run in progress — 4 of the last 6")).toBeInTheDocument();
  });

  it("only alarms at-risk survival once it drops to a quarter", () => {
    const view = fixture();
    view.draft.my_next_picks = [30, 55];
    view.available = view.available.slice(0, 2);
    view.available[0].survival_next = 0.1;
    view.available[1].survival_next = 0.4;

    render(<SidePanel view={view} />);
    expect(screen.getByText("10%")).toHaveClass("is-low");
    expect(screen.getByText("40%")).not.toHaveClass("is-low");
    expect(screen.getByText("40%")).toHaveClass("mid");
  });

  it("prompts for a username instead of showing an empty roster", () => {
    const view = fixture();
    view.my_roster = null;

    render(<SidePanel view={view} />);
    expect(
      screen.getByText("Set your Sleeper username to track your team."),
    ).toBeInTheDocument();
  });
});
