import { render, screen, within } from "@testing-library/react";
import userEvent from "@testing-library/user-event";
import { describe, expect, it, vi } from "vitest";
import type { AvailablePlayer } from "../types";
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

const rowNames = () =>
  screen
    .getAllByRole("row")
    .slice(1)
    .map((row) => within(row).getAllByRole("cell")[1].textContent);

describe("Board", () => {
  // Dogfood pass 2, ISSUE-P2-003: rows carried an `injured` class with no rule
  // behind it — markup that promises styling and delivers none.
  it("marks a flagged player with a badge and nothing else", () => {
    render(
      <Board
        players={[player("a", "Alpha", "WR", { injury_status: "Out" })]}
        positions={["WR"]}
        onDraft={vi.fn()}
      />,
    );
    expect(screen.getByText("Out")).toBeInTheDocument();
    expect(document.querySelectorAll(".board tbody tr.injured")).toHaveLength(0);
  });

  // Dogfood ISSUE-010: 11 columns and 200 rows with no way for a screen
  // reader to tie "T4" to "Tier".
  it("names its columns for assistive technology", () => {
    render(
      <Board players={[player("a", "Alpha", "WR")]} positions={["WR"]} onDraft={vi.fn()} />,
    );
    const table = screen.getByRole("table");
    expect(table).toHaveAccessibleName(/available players/i);
    for (const header of screen.getAllByRole("columnheader")) {
      expect(header).toHaveAttribute("scope", "col");
    }
  });

  // Dogfood ISSUE-011.
  it("offers no draft action once the draft is complete", () => {
    render(
      <Board
        players={[player("a", "Alpha", "WR"), player("b", "Bravo", "RB")]}
        positions={["WR", "RB"]}
        onDraft={vi.fn()}
        draftOver
      />,
    );
    const buttons = screen.getAllByRole("button", { name: "Draft" });
    expect(buttons).toHaveLength(2);
    for (const button of buttons) expect(button).toBeDisabled();
    expect(buttons[0]).toHaveAttribute("title", "The draft is complete");
  });

  it("builds position filters from league data, including kicker", async () => {
    const user = userEvent.setup();
    render(
      <Board
        players={[
          player("qb", "Quarterback", "QB"),
          player("k", "Kicker", "K"),
        ]}
        positions={["QB", "K"]}
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
        onDraft={vi.fn()}
      />,
    );

    await user.type(screen.getByRole("textbox", { name: "Search players" }), "missing");
    expect(screen.getByText("No matching players")).toBeInTheDocument();
    expect(screen.getByText("0 players")).toBeInTheDocument();
  });

  it("sorts by a clicked column, flips on a second click, and puts blanks last", async () => {
    const user = userEvent.setup();
    render(
      <Board
        players={[
          player("a", "Alpha", "WR", { overall_rank: 1, bye_week: 9 }),
          player("b", "Bravo", "WR", { overall_rank: 2, bye_week: null }),
          player("c", "Charlie", "WR", { overall_rank: 3, bye_week: 5 }),
          player("d", "Delta", "WR", { overall_rank: 4, bye_week: 7 }),
        ]}
        positions={["WR"]}
        onDraft={vi.fn()}
      />,
    );
    // Default: overall rank.
    expect(rowNames()).toEqual(["Alpha", "Bravo", "Charlie", "Delta"]);
    expect(screen.getByRole("columnheader", { name: "#" })).toHaveAttribute("aria-sort", "ascending");

    await user.click(screen.getByRole("button", { name: "Bye" }));
    expect(rowNames()).toEqual(["Charlie", "Delta", "Alpha", "Bravo"]);
    expect(screen.getByRole("columnheader", { name: "Bye" })).toHaveAttribute("aria-sort", "ascending");
    expect(screen.getByRole("columnheader", { name: "#" })).not.toHaveAttribute("aria-sort");

    await user.click(screen.getByRole("button", { name: "Bye" }));
    expect(rowNames()).toEqual(["Alpha", "Delta", "Charlie", "Bravo"]);
    expect(screen.getByRole("columnheader", { name: "Bye" })).toHaveAttribute("aria-sort", "descending");

    // Back to the default order via the rank column.
    await user.click(screen.getByRole("button", { name: "#" }));
    expect(rowNames()).toEqual(["Alpha", "Bravo", "Charlie", "Delta"]);
  });

  it("points and VORP sort highest-first on the first click", async () => {
    const user = userEvent.setup();
    render(
      <Board
        players={[
          player("a", "Alpha", "RB", { overall_rank: 1, points: 200, vorp: 40 }),
          player("b", "Bravo", "RB", { overall_rank: 2, points: 260, vorp: 30 }),
          player("c", "Charlie", "RB", { overall_rank: 3, points: 230, vorp: 55 }),
        ]}
        positions={["RB"]}
        onDraft={vi.fn()}
      />,
    );
    await user.click(screen.getByRole("button", { name: "Pts" }));
    expect(rowNames()).toEqual(["Bravo", "Charlie", "Alpha"]);
    await user.click(screen.getByRole("button", { name: "VORP" }));
    expect(rowNames()).toEqual(["Charlie", "Alpha", "Bravo"]);
    // Sorting survives a position filter change and the search box.
    await user.type(screen.getByRole("textbox", { name: "Search players" }), "a");
    expect(rowNames()).toEqual(["Charlie", "Alpha", "Bravo"]);
  });

  it("shows a preseason Questionable tag quietly and a real one loudly", () => {
    render(
      <Board
        players={[
          player("q", "Quiet", "WR", { injury_status: "Questionable" }),
          player("o", "Loud", "WR", { injury_status: "Out" }),
        ]}
        positions={["WR"]}
        onDraft={vi.fn()}
      />,
    );
    expect(screen.getByText("Questionable")).toHaveClass("injury", "mild");
    expect(screen.getByText("Out")).toHaveClass("injury");
    expect(screen.getByText("Out")).not.toHaveClass("mild");
  });
});
