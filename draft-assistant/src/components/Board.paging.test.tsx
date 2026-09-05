import { fireEvent, render, within } from "@testing-library/react";
import { describe, expect, it, vi } from "vitest";
import { boardPlayer as player } from "../test/boardPlayer";
import { Board } from "./Board";

/** 450 players, so the board opens on the first page of 200 and has two more
 *  pages behind it. Every name is unique and searchable. */
function pool() {
  return Array.from({ length: 450 }, (_, i) =>
    player(`p${i}`, `Player ${String(i).padStart(3, "0")}`, i % 2 === 0 ? "RB" : "WR", {
      points: 1000 - i,
    }),
  );
}

function board() {
  const view = render(
    <Board
      players={pool()}
      positions={["RB", "WR"]}
      loading={false}
      boardSize={450}
      onDraft={vi.fn()}
    />,
  );
  const rows = () => view.container.querySelectorAll(".board-body").length;
  // `getByRole` builds the accessibility tree of whatever it is given, and
  // what it was being given here is a board of 400 rows: every query walked
  // several thousand nodes, and the two tests below took three and four
  // seconds on an idle machine and blew the five-second budget under parallel
  // worker load. The controls and the paging footer are a handful of nodes
  // each, so scoping the queries to them is the same assertion at a fraction
  // of the cost.
  const controls = () => within(view.container.querySelector(".board-controls") as HTMLElement);
  const foot = () => within(view.container.querySelector(".board-foot") as HTMLElement);
  return { ...view, rows, controls, foot };
}

// The page size was only ever put back by the "Show first 200" button — and
// that button is drawn only while there is a page still hidden. Page out to
// 400 rows, then search for one player, and the board committed all 400 rows
// it could still match while the button that would have undone it was gone.
describe("Board paging across a filter change", () => {
  it("goes back to the first page when the search changes", () => {
    const { rows, controls, foot } = board();
    expect(rows()).toBe(200);

    fireEvent.click(foot().getByRole("button", { name: /^Show 200 more$/ }));
    expect(rows()).toBe(400);

    // "Player 1" matches 100 of them; the board must show a page, not all of
    // the ones a stale limit would have allowed.
    fireEvent.change(controls().getByLabelText("Search players"), {
      target: { value: "Player 0" },
    });
    expect(rows()).toBe(100);
    expect(foot().queryByRole("button", { name: /^Show first 200$/ })).toBeNull();

    fireEvent.change(controls().getByLabelText("Search players"), { target: { value: "" } });
    expect(rows()).toBe(200);
  });

  it("goes back to the first page when the position tab changes", () => {
    const { rows, controls, foot } = board();
    fireEvent.click(foot().getByRole("button", { name: /^Show 200 more$/ }));
    expect(rows()).toBe(400);

    // 225 running backs: a page of them, and a "show more" for the rest.
    fireEvent.click(controls().getByRole("button", { name: "RB" }));
    expect(rows()).toBe(200);
    expect(foot().getByRole("button", { name: /^Show 25 more$/ })).toBeInTheDocument();
  });
});
