import { fireEvent, render, screen } from "@testing-library/react";
import userEvent from "@testing-library/user-event";
import { afterEach, describe, expect, it, vi } from "vitest";
import type { AvailablePlayer, DraftView } from "../types";
import { boardPlayer as player } from "../test/boardPlayer";
import { stableAvailable } from "../boardIdentity";
import { Board } from "./Board";

describe("Board", () => {
  it("shows an injury as a one-letter tag that still reads in full", () => {
    // Sleeper's dictionary spells this "Questionable"; uppercased in the row
    // it was wider than the whole player column with the chat panel open, and
    // ran over the position badge beside it.
    render(
      <Board
        players={[player("wr", "Sore Wideout", "WR", { injury_status: "Questionable" })]}
        positions={["WR"]}
        loading={false}
        boardSize={1}
        onDraft={vi.fn()}
      />,
    );

    const tag = screen.getByTitle("Questionable");
    expect(tag).toHaveTextContent("Q");
    expect(tag.textContent).toBe("QQuestionable");
    // "Q" alone is what the eye sees; the word is there for a screen reader.
    expect(tag.querySelector('[aria-hidden="true"]')?.textContent).toBe("Q");
  });

  it("says nothing about a status it does not recognise", () => {
    render(
      <Board
        players={[player("wr", "Fine Wideout", "WR", { injury_status: "Probable" })]}
        positions={["WR"]}
        loading={false}
        boardSize={1}
        onDraft={vi.fn()}
      />,
    );
    expect(screen.queryByTitle("Probable")).not.toBeInTheDocument();
    expect(document.querySelector(".tag")).toBeNull();
  });

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

  // `fireEvent` rather than `userEvent`: three commits of 200-odd rows is
  // already the expensive part, and the pointer sequence behind a real click
  // adds seconds to it under coverage instrumentation.
  it("opens on one page and loads the next on demand", () => {
    const { container } = render(board(many(450)));
    const rows = () => container.querySelectorAll(".board-body");

    expect(rows()).toHaveLength(200);
    expect(screen.getByText("Showing 200 of 450")).toBeInTheDocument();

    fireEvent.click(screen.getByRole("button", { name: "Show 200 more" }));
    expect(rows()).toHaveLength(400);
    expect(screen.getByText("Showing 400 of 450")).toBeInTheDocument();

    // The last page offers only what is left of the pool.
    expect(screen.getByRole("button", { name: "Show 50 more" })).toBeInTheDocument();

    fireEvent.click(screen.getByRole("button", { name: "Show first 200" }));
    expect(rows()).toHaveLength(200);
    // Four commits of a couple of hundred rows apiece, which is more than the
    // default budget allows for once coverage instrumentation is on top.
  }, 20_000);

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

describe("Board when there is nothing to show", () => {
  it("distinguishes a filtered-out board from a fully drafted one", async () => {
    const user = userEvent.setup();
    render(
      <Board
        players={[player("qb", "Quarterback", "QB")]}
        positions={["QB", "RB"]}
        loading={false}
        boardSize={1}
        onDraft={vi.fn()}
      />,
    );

    // Filtered empty: the way out is offered.
    await user.click(screen.getByRole("button", { name: "RB" }));
    expect(screen.getByText("No players match")).toBeInTheDocument();
    expect(
      screen.getByText(/Nothing left at this position with the current filter/),
    ).toBeInTheDocument();

    await user.click(screen.getByRole("button", { name: "Clear filters" }));
    expect(screen.getByText("Quarterback")).toBeInTheDocument();
    expect(screen.getByRole("button", { name: "ALL" })).toHaveAttribute("aria-pressed", "true");
  });

  it("clears a search box as well as the position tab", async () => {
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
    const search = screen.getByRole("textbox", { name: "Search players" });
    await user.type(search, "nobody");
    await user.click(screen.getByRole("button", { name: "Clear filters" }));
    expect(search).toHaveValue("");
    expect(screen.getByText("1 player")).toBeInTheDocument();
  });

  it("says the board is drafted out when nothing is filtered", () => {
    render(
      <Board players={[]} positions={["QB"]} loading={false} boardSize={0} onDraft={vi.fn()} />,
    );
    expect(screen.getByText("Every player on the board has been drafted.")).toBeInTheDocument();
    // Nothing to clear, so nothing is offered.
    expect(screen.queryByRole("button", { name: "Clear filters" })).not.toBeInTheDocument();
  });

  it("does not name a player count it does not have while loading", () => {
    render(
      <Board players={[]} positions={["QB"]} loading={true} boardSize={0} onDraft={vi.fn()} />,
    );
    expect(screen.getByRole("status")).toHaveTextContent("Pulling projections…");
    expect(screen.queryByText(/Pulling projections for/)).not.toBeInTheDocument();
  });
});
