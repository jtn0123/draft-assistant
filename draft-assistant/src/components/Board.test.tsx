import { render, screen } from "@testing-library/react";
import userEvent from "@testing-library/user-event";
import { afterEach, describe, expect, it, vi } from "vitest";
import type { AvailablePlayer, DraftView } from "../types";
import { stableAvailable } from "../boardIdentity";
import { Board } from "./Board";

function player(
  id: string,
  name: string,
  position: string,
  over: Partial<AvailablePlayer> = {},
): AvailablePlayer {
  return {
    player_id: id,
    name,
    position,
    team: null,
    bye_week: null,
    points: 100,
    bonus_points: 0,
    vorp: 10,
    tier: 1,
    position_rank: 1,
    overall_rank: 1,
    adp: 20,
    injury_status: null,
    sleeper_pts_ppr: null,
    survival_next: 0.5,
    ...over,
  };
}

describe("Board", () => {
  it("builds position filters from league data, including kicker", async () => {
    const user = userEvent.setup();
    render(
      <Board
        players={[player("qb", "Quarterback", "QB"), player("k", "Kicker", "K")]}
        positions={["QB", "K"]}
        loading={false}
        boardSize={2}
        onDraft={vi.fn()}
      />,
    );

    expect(screen.queryByRole("button", { name: "DEF" })).not.toBeInTheDocument();
    await user.click(screen.getByRole("button", { name: "K" }));
    expect(screen.getByText("Kicker")).toBeInTheDocument();
    expect(screen.queryByText("Quarterback")).not.toBeInTheDocument();
  });

  it("explains an empty search result", async () => {
    const user = userEvent.setup();
    render(
      <Board
        players={[player("qb", "Quarterback", "QB")]}
        positions={["QB"]}
        loading={false}
        boardSize={1}
        onDraft={vi.fn()}
      />,
    );

    await user.type(screen.getByRole("textbox", { name: "Search players" }), "missing");
    expect(screen.getByText("No players match")).toBeInTheDocument();
    expect(screen.getByText("0 players")).toBeInTheDocument();
  });

  it("jumps to search on '/' unless already typing somewhere", async () => {
    const user = userEvent.setup();
    render(
      <>
        <input aria-label="Other field" />
        <Board
          players={[player("qb", "Quarterback", "QB")]}
          positions={["QB"]}
          loading={false}
          boardSize={1}
          onDraft={vi.fn()}
        />
      </>,
    );
    const search = screen.getByRole("textbox", { name: "Search players" });
    expect(search).toHaveAttribute("placeholder", "Search players — press /");

    await user.keyboard("/");
    expect(search).toHaveFocus();
    // The shortcut key itself must not land in the box.
    expect(search).toHaveValue("");

    const other = screen.getByRole("textbox", { name: "Other field" });
    await user.click(other);
    await user.keyboard("/");
    expect(other).toHaveFocus();
    expect(other).toHaveValue("/");
  });

  it("names the player count while projections load", () => {
    render(
      <Board players={[]} positions={["QB"]} loading={true} boardSize={312} onDraft={vi.fn()} />,
    );
    expect(screen.getByText("Pulling projections for 312 players…")).toBeInTheDocument();
  });
});

// Grade item G7. The filter-and-sort memo is keyed on the players array, and
// `applyView` now recycles that array when an incoming view says the same
// thing about the pool. These tests hold both ends of the bargain: no work
// when the data is unchanged, and never a stale number when it is not.
describe("Board across repeated updates", () => {
  afterEach(() => vi.restoreAllMocks());

  /** What `applyView` does to the pool, without the rest of the app. */
  const deliver = (prev: AvailablePlayer[], next: AvailablePlayer[]): AvailablePlayer[] =>
    stableAvailable({ available: prev } as DraftView, { available: next } as DraftView).available;

  const pool = () => [
    player("a", "Alpha", "RB", { points: 210, vorp: 40, overall_rank: 1 }),
    player("b", "Bravo", "WR", { points: 190, vorp: 30, overall_rank: 2 }),
    player("c", "Charlie", "RB", { points: 175, vorp: 22, overall_rank: 3 }),
  ];

  const board = (players: AvailablePlayer[]) => (
    <Board
      players={players}
      positions={["RB", "WR"]}
      loading={false}
      boardSize={players.length}
      onDraft={vi.fn()}
    />
  );

  const rows = (container: HTMLElement) => [...container.querySelectorAll(".board-body")];
  const names = (container: HTMLElement) =>
    rows(container).map((row) => row.querySelector(".board-player .ellipsis")?.textContent);
  const points = (container: HTMLElement) =>
    rows(container).map((row) => row.children[5].textContent);

  it("shows the new projections when a rebuilt board keeps the same players", () => {
    const before = pool();
    const { container, rerender } = render(board(before));
    // The board opens sorted by points, so new projections have to re-order it.
    expect(names(container)).toEqual(["Alpha", "Bravo", "Charlie"]);
    expect(points(container)).toEqual(["210", "190", "175"]);

    // "Refresh data": same players, same ids, same order in — new numbers.
    const after = deliver(before, [
      { ...before[0], points: 150, vorp: 12 },
      { ...before[1], points: 240, vorp: 55 },
      { ...before[2], points: 175, vorp: 22 },
    ]);
    expect(after).not.toBe(before);
    rerender(board(after));

    expect(names(container)).toEqual(["Bravo", "Charlie", "Alpha"]);
    expect(points(container)).toEqual(["240", "175", "150"]);
  });

  it("does no work when an update carries an identical pool", () => {
    const sorts = vi.spyOn(Array.prototype, "sort");
    const first = pool();
    const { container, rerender } = render(board(first));
    expect(sorts).toHaveBeenCalled();

    // A poll tick: a brand-new array of brand-new objects saying the same thing.
    const tick = deliver(
      first,
      first.map((p) => ({ ...p })),
    );
    expect(tick).toBe(first);

    const sortsBefore = sorts.mock.calls.length;
    rerender(board(tick));
    expect(sorts.mock.calls.length).toBe(sortsBefore);
    expect(names(container)).toEqual(["Alpha", "Bravo", "Charlie"]);

    // …and an update that takes a drafted player off the board does re-sort.
    const third = deliver(first, [first[0], first[2]]);
    rerender(board(third));
    expect(sorts.mock.calls.length).toBeGreaterThan(sortsBefore);
    expect(names(container)).toEqual(["Alpha", "Charlie"]);
  });
});

// Grade item G7. "Show all" used to drop the page cap and commit the entire
// pool at once — hundreds of rows, each with two names, a headshot carrying
// its own state, effect and store subscription, and a logo.
describe("Board paging", () => {
  const many = (n: number) =>
    Array.from({ length: n }, (_, i) =>
      player(`p${i}`, `Player ${i}`, "RB", { points: 1000 - i, overall_rank: i + 1 }),
    );

  const board = (players: AvailablePlayer[]) => (
    <Board
      players={players}
      positions={["RB"]}
      loading={false}
      boardSize={players.length}
      onDraft={vi.fn()}
    />
  );

  it("opens on one page and loads the next on demand", async () => {
    const user = userEvent.setup();
    const { container } = render(board(many(450)));
    const rows = () => container.querySelectorAll(".board-body");

    expect(rows()).toHaveLength(200);
    expect(screen.getByText("Showing 200 of 450")).toBeInTheDocument();

    await user.click(screen.getByRole("button", { name: "Show 200 more" }));
    expect(rows()).toHaveLength(400);
    expect(screen.getByText("Showing 400 of 450")).toBeInTheDocument();

    // The last page is only as big as what is left.
    await user.click(screen.getByRole("button", { name: "Show 50 more" }));
    expect(rows()).toHaveLength(450);
    expect(screen.getByText("450 players")).toBeInTheDocument();
    expect(screen.queryByRole("button", { name: /more/ })).not.toBeInTheDocument();

    await user.click(screen.getByRole("button", { name: "Show first 200" }));
    expect(rows()).toHaveLength(200);
  });

  it("leaves the paging control out when everything already fits", () => {
    render(board(many(3)));
    expect(screen.queryByRole("button", { name: /more/ })).not.toBeInTheDocument();
    expect(screen.queryByRole("button", { name: /Show first/ })).not.toBeInTheDocument();
  });

  it("announces the skeleton rather than leaving the wait silent", () => {
    render(
      <Board players={[]} positions={["QB"]} loading={true} boardSize={312} onDraft={vi.fn()} />,
    );
    expect(screen.getByRole("status")).toHaveTextContent("Pulling projections for 312 players…");
  });
});
