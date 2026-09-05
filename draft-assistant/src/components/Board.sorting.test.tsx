import { fireEvent, render, screen } from "@testing-library/react";
import { describe, expect, it, vi } from "vitest";
import { boardPlayer as player } from "../test/boardPlayer";
import { Board } from "./Board";

// Grade item D6. Every column is sortable and every column head is a control
// a user clicks; before this, only the default points sort was ever exercised,
// so nine of the ten value accessors and the whole direction-toggling path
// were dark.
describe("Board sorting", () => {
  const pool = () => [
    player("a", "Alpha", "WR", {
      team: "SF",
      bye_week: 9,
      points: 100,
      vorp: 5,
      tier: 3,
      adp: 30,
      overall_rank: 3,
      survival_next: 0.1,
    }),
    player("b", "Bravo", "RB", {
      team: null,
      bye_week: null,
      points: 200,
      vorp: 15,
      tier: 1,
      adp: 10,
      overall_rank: 1,
      survival_next: 0.9,
    }),
    player("c", "Charlie", "QB", {
      team: "KC",
      bye_week: 5,
      points: 150,
      vorp: 10,
      tier: 2,
      adp: 20,
      overall_rank: 2,
      survival_next: null,
    }),
  ];

  function board() {
    const view = render(
      <Board
        players={pool()}
        positions={["QB", "RB", "WR"]}
        loading={false}
        boardSize={3}
        onDraft={vi.fn()}
      />,
    );
    const names = () =>
      [...view.container.querySelectorAll(".board-body")].map(
        (row) => row.querySelector(".board-player .ellipsis")?.textContent,
      );
    /** Click a column head by its label, whatever its sort state is called. */
    const sortBy = (label: string) =>
      fireEvent.click(screen.getByRole("button", { name: new RegExp(`^${label}, `) }));
    return { ...view, names, sortBy };
  }

  it("orders by each column in the direction that column naturally reads", () => {
    const { names, sortBy } = board();
    // Points is the board's opening sort, high to low.
    expect(names()).toEqual(["Bravo", "Charlie", "Alpha"]);

    sortBy("#");
    expect(names()).toEqual(["Bravo", "Charlie", "Alpha"]);
    sortBy("Player");
    expect(names()).toEqual(["Alpha", "Bravo", "Charlie"]);
    sortBy("Pos");
    expect(names()).toEqual(["Charlie", "Bravo", "Alpha"]);
    sortBy("Tier");
    expect(names()).toEqual(["Bravo", "Charlie", "Alpha"]);
    sortBy("Adp");
    expect(names()).toEqual(["Bravo", "Charlie", "Alpha"]);
    sortBy("Surv");
    expect(names()).toEqual(["Alpha", "Bravo", "Charlie"]);
    // Counting stats open high-to-low, names and ranks low-to-high.
    sortBy("Vorp");
    expect(names()).toEqual(["Bravo", "Charlie", "Alpha"]);
  });

  // A blank Team, Bye or Surv used to ride the sort direction: `compare`
  // returned +1 for a missing value and the caller multiplied by the sign, so
  // flipping the column sent every free agent and every no-bye player to the
  // top of the board — over the players you were actually reading.
  it("keeps blanks at the bottom whichever way a column is pointing", () => {
    const { names, sortBy } = board();

    // Bravo has no team; Charlie (KC) and Alpha (SF) do.
    sortBy("Team");
    expect(names()).toEqual(["Charlie", "Alpha", "Bravo"]);
    sortBy("Team");
    expect(names()).toEqual(["Alpha", "Charlie", "Bravo"]);

    // Bravo has no bye week either.
    sortBy("Bye");
    expect(names()[2]).toBe("Bravo");
    sortBy("Bye");
    expect(names()[2]).toBe("Bravo");

    // Charlie is the one with no survival number.
    sortBy("Surv");
    expect(names()[2]).toBe("Charlie");
    sortBy("Surv");
    expect(names()[2]).toBe("Charlie");
  });

  it("flips direction when the same column is clicked again, and says which", () => {
    const { names, sortBy } = board();
    sortBy("Player");
    expect(names()).toEqual(["Alpha", "Bravo", "Charlie"]);
    expect(screen.getByText(/Sorted by name, low to high/)).toBeInTheDocument();
    // The head names its own state, since a grid of buttons has no aria-sort.
    expect(screen.getByRole("button", { name: "Player, sorted ascending" })).toBeInTheDocument();

    sortBy("Player");
    expect(names()).toEqual(["Charlie", "Bravo", "Alpha"]);
    expect(screen.getByText(/Sorted by name, high to low/)).toBeInTheDocument();
    expect(screen.getByRole("button", { name: "Player, sorted descending" })).toBeInTheDocument();

    // Moving to another column starts from that column's own direction rather
    // than carrying the previous one over.
    sortBy("Pts");
    expect(names()).toEqual(["Bravo", "Charlie", "Alpha"]);
    expect(screen.getByText(/Sorted by points, high to low/)).toBeInTheDocument();
  });

  it("puts players with no team or no bye week after the ones that have them", () => {
    const { names, sortBy } = board();
    // Bravo is the free agent with no bye — a blank must not read as zero and
    // take the top of the board.
    sortBy("Team");
    expect(names()).toEqual(["Charlie", "Alpha", "Bravo"]);
    sortBy("Bye");
    expect(names()).toEqual(["Charlie", "Alpha", "Bravo"]);
  });

  it("falls back to points when the column it was sorting by disappears", () => {
    // The imported column exists only while a projections CSV is loaded. A
    // rebuild without one used to leave the board unsorted while the footer
    // went on claiming the imported rank had ordered it.
    const imported = (rank: number) => ({
      positional_rank: rank,
      overall_rank: rank,
      source: "Clay",
    });
    const withOpinions = [
      // The two orders disagree, so which one is in force is visible.
      player("a", "Alpha", "WR", { points: 300, second_opinion: imported(3) }),
      player("b", "Bravo", "RB", { points: 100, second_opinion: imported(1) }),
      player("c", "Charlie", "QB", { points: 200, second_opinion: imported(2) }),
    ];
    const view = render(
      <Board
        players={withOpinions}
        positions={["QB", "RB", "WR"]}
        loading={false}
        boardSize={3}
        onDraft={vi.fn()}
      />,
    );
    const names = () =>
      [...view.container.querySelectorAll(".board-body")].map(
        (row) => row.querySelector(".board-player .ellipsis")?.textContent,
      );

    fireEvent.click(screen.getByRole("button", { name: /^Clay, / }));
    expect(names()).toEqual(["Bravo", "Charlie", "Alpha"]);
    expect(screen.getByText(/Sorted by the imported rank, low to high/)).toBeInTheDocument();

    // The CSV is gone, and the column with it.
    view.rerender(
      <Board
        players={withOpinions.map((p) => ({ ...p, second_opinion: null }))}
        positions={["QB", "RB", "WR"]}
        loading={false}
        boardSize={3}
        onDraft={vi.fn()}
      />,
    );
    expect(screen.queryByRole("button", { name: /^Clay, / })).toBeNull();
    expect(screen.getByText(/Sorted by points, high to low/)).toBeInTheDocument();
    expect(names()).toEqual(["Alpha", "Charlie", "Bravo"]);
  });

  it("shows a dash and no colour for a survival chance nobody can compute", () => {
    const { container, sortBy } = board();
    sortBy("Surv");
    const survival = [...container.querySelectorAll(".board-body")].map((row) => row.children[9]);
    expect(survival.map((c) => c.textContent)).toEqual(["10%", "90%", "–"]);
    expect(survival[0].className).toContain("surv-low");
    expect(survival[1].className).toContain("surv-high");
    expect(survival[2].className).toContain("muted");
  });
});

// The board fell back to points for the render but left `sortKey` naming the
// column that had gone. Clicking the Pts head then counted as "you are already
// on this column, flip it", the flip landed on a direction the render was not
// reading, and the header the footer pointed at was the one head on the board
// that did nothing at all.
describe("Board sorting after the imported column goes away", () => {
  const imported = (rank: number) => ({
    positional_rank: rank,
    overall_rank: rank,
    source: "Clay",
  });
  const pool = (withOpinion: boolean) => [
    player("a", "Alpha", "WR", {
      points: 300,
      second_opinion: withOpinion ? imported(3) : null,
    }),
    player("b", "Bravo", "RB", {
      points: 100,
      second_opinion: withOpinion ? imported(1) : null,
    }),
    player("c", "Charlie", "QB", {
      points: 200,
      second_opinion: withOpinion ? imported(2) : null,
    }),
  ];

  it("lets the fallback column be clicked once the import is gone", () => {
    const props = (withOpinion: boolean) => ({
      players: pool(withOpinion),
      positions: ["QB", "RB", "WR"] as const,
      loading: false,
      boardSize: 3,
      onDraft: vi.fn(),
    });
    const view = render(<Board {...props(true)} positions={["QB", "RB", "WR"]} />);
    const names = () =>
      [...view.container.querySelectorAll(".board-body")].map(
        (row) => row.querySelector(".board-player .ellipsis")?.textContent,
      );

    fireEvent.click(screen.getByRole("button", { name: /^Clay, / }));
    expect(names()).toEqual(["Bravo", "Charlie", "Alpha"]);

    view.rerender(<Board {...props(false)} positions={["QB", "RB", "WR"]} />);
    expect(names()).toEqual(["Alpha", "Charlie", "Bravo"]);

    // The board now says it is sorted by points, high to low, so clicking the
    // Pts head has to turn it around.
    fireEvent.click(screen.getByRole("button", { name: /^Pts, / }));
    expect(names()).toEqual(["Bravo", "Charlie", "Alpha"]);
    expect(screen.getByText(/Sorted by points, low to high/)).toBeInTheDocument();
  });
});
