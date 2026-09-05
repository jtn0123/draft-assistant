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

  it("counts a back-to-back snake turn as one window, as the backend does", () => {
    const view = fixture();
    view.draft.teams = 12;
    view.draft.current_pick = 12;
    view.draft.is_my_pick = true;
    // The turn at the end of round one: picks 12 and 13 are the same window,
    // with nobody picking in between. Pricing survival against 13 said
    // everyone survives, which read the board's most dangerous moment as its
    // safest — `view_signals::survival_target` picks 36, and so must this.
    view.draft.my_next_picks = [12, 13, 36, 37];

    render(<SidePanel view={view} />);
    expect(screen.getByText("Won't last to 3.12")).toBeInTheDocument();
    expect(screen.queryByText("Won't last to 2.01")).not.toBeInTheDocument();
  });

  it("counts three picks in a row as one window", () => {
    const view = fixture();
    view.draft.teams = 14;
    view.draft.current_pick = 27;
    view.draft.is_my_pick = true;
    // A traded pick leaves me 27, 28 and 29 back to back. Every one of them
    // is the same window, so the pick that matters is 55.
    view.draft.my_next_picks = [27, 28, 29, 55];

    render(<SidePanel view={view} />);
    expect(screen.getByText("Won't last to 4.13")).toBeInTheDocument();
    expect(screen.queryByText("Won't last to 2.14")).not.toBeInTheDocument();
  });

  it("treats a keeper between two of my picks as no gap at all", () => {
    const view = fixture();
    view.draft.teams = 14;
    view.draft.current_pick = 27;
    view.draft.is_my_pick = true;
    // Picks 28 and 29 are keepers, already in the book: nobody selects there,
    // so 27 and 30 are adjacent and 55 is the pick survival is judged at.
    // Reading 30 as "my next turn" said every player was 99% to last.
    view.draft.my_next_picks = [27, 30, 55];
    view.draft.keeper_picks = [28, 29];

    render(<SidePanel view={view} />);
    expect(screen.getByText("Won't last to 4.13")).toBeInTheDocument();
    expect(screen.queryByText("Won't last to 3.02")).not.toBeInTheDocument();
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

  it("writes the VORP a player takes with him as one loss, however it is signed", () => {
    const view = fixture();
    view.draft.teams = 14;
    view.draft.is_my_pick = true;
    view.draft.my_next_picks = [27, 30, 55];
    // Below replacement, which the board does say about the back of the pool.
    // The row shows what his going costs, so the two minus signs the old
    // markup produced — "−-4" — were never a number.
    const risky = view.available.find((p) => (p.survival_next ?? 1) < 0.5);
    if (risky === undefined) throw new Error("the fixture has an at-risk player");
    risky.vorp = -4;

    const { container } = render(<SidePanel view={view} />);
    const written = [...container.querySelectorAll(".risk-row .num")].map((n) => n.textContent);
    expect(written).toContain("−4");
    expect(written.some((t) => t?.includes("-"))).toBe(false);
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

describe("SidePanel pick market", () => {
  it("prices every round the draft has learned, cheapest last", () => {
    const view = fixture();
    view.pick_prices = [
      { round: 1, points: 82.4, example: "Alpha Back" },
      { round: 2, points: 40, example: "Beta Wideout" },
      { round: 3, points: 40, example: null },
    ];

    render(<SidePanel view={view} />);
    expect(screen.getByText("Pick market")).toBeInTheDocument();
    const rows = [...document.querySelectorAll(".price-row")];
    expect(rows.map((row) => row.textContent)).toEqual([
      "R1Alpha Back82",
      "R2Beta Wideout40",
      "R3\u201440",
    ]);
    // Said in full on the row itself, so the number cannot be read as a
    // projection.
    expect(rows[0].getAttribute("title")).toMatch(/median VORP taken in this round/);
  });

  it("stays away entirely until the draft has rounds to learn from", () => {
    const view = fixture();
    view.pick_prices = [];
    render(<SidePanel view={view} />);
    expect(screen.queryByText("Pick market")).not.toBeInTheDocument();
  });

  it("survives a view captured before pick pricing existed", () => {
    const view = fixture();
    delete view.pick_prices;
    render(<SidePanel view={view} />);
    expect(screen.queryByText("Pick market")).not.toBeInTheDocument();
  });
});
