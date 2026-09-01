import { render, screen } from "@testing-library/react";
import { afterEach, describe, expect, it, vi } from "vitest";
import fixtureJson from "../../public/dev-fixture.json";
import type { DraftView } from "../types";
import { stableAvailable } from "../boardIdentity";
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

  it("marks a kept player on my roster so the round beside them makes sense", () => {
    const view = fixture();
    const roster = view.my_roster;
    if (roster === null) throw new Error("the fixture has a roster");
    roster.players[0].round = 13;
    roster.players[0].is_keeper = true;

    render(<SidePanel view={view} />);
    // A 13th-round player in the first round of the draft is only explicable
    // as a keeper, so the roster says so.
    const tag = screen.getByTitle("Kept from last season");
    expect(tag).toHaveTextContent("R13 · K");
    // Everyone else is drafted, and carries no tag.
    expect(screen.getAllByTitle("Kept from last season")).toHaveLength(1);
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
    expect(screen.getByText("Set your Sleeper username to track your team.")).toBeInTheDocument();
  });
});

// Grade item G7. The at-risk list filters, sorts and slices several hundred
// players, on a panel that re-renders on every poll and every tick of the
// pick clock. `applyView` recycles the pool's array identity when nothing
// about it changed (boardIdentity.ts); this is the memo that spends it.
describe("SidePanel across repeated updates", () => {
  afterEach(() => vi.restoreAllMocks());

  const risky = (container: HTMLElement): (string | null)[] =>
    [...container.querySelectorAll(".risk-row .ellipsis")].map((n) => n.textContent);

  it("does not rebuild the at-risk list when an update leaves the pool alone", () => {
    const view = fixture();
    const sorts = vi.spyOn(Array.prototype, "sort");
    const { container, rerender } = render(<SidePanel view={view} />);
    const sortsBefore = sorts.mock.calls.length;
    expect(sortsBefore).toBeGreaterThan(0);
    const listed = risky(container);
    expect(listed.length).toBeGreaterThan(0);

    // A poll tick: a brand-new pool of brand-new objects saying the same thing.
    const tick = stableAvailable(view, {
      ...view,
      available: view.available.map((p) => ({ ...p })),
    });
    expect(tick.available).toBe(view.available);

    rerender(<SidePanel view={tick} />);
    expect(sorts.mock.calls.length).toBe(sortsBefore);
    expect(risky(container)).toEqual(listed);
  });

  it("rebuilds it when survival actually moves", () => {
    const view = fixture();
    view.available = view.available.slice(0, 3);
    view.available[0].survival_next = 0.4;
    view.available[1].survival_next = 0.1;
    view.available[2].survival_next = 0.2;
    const names = view.available.map((p) => p.name);

    const { container, rerender } = render(<SidePanel view={view} />);
    expect(risky(container)).toEqual([names[1], names[2], names[0]]);

    const moved = stableAvailable(view, {
      ...view,
      available: view.available.map((p, i) => ({ ...p, survival_next: [0.05, 0.9, 0.2][i] })),
    });
    expect(moved.available).not.toBe(view.available);
    rerender(<SidePanel view={moved} />);
    expect(risky(container)).toEqual([names[0], names[2]]);
  });
});
