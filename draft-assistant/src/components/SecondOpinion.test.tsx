// The board's imported second-opinion column: present only when something was
// imported, tinted only when the two boards really disagree.

import { fireEvent, render, screen } from "@testing-library/react";
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
    team: "SF",
    bye_week: 9,
    points: 100,
    bonus_points: 0,
    vorp: 10,
    tier: 1,
    position_rank: 21,
    overall_rank: 40,
    adp: 20,
    injury_status: null,
    sleeper_pts_ppr: null,
    second_opinion: null,
    survival_next: 0.5,
    ...over,
  };
}

/** A player the imported source ranks `csvRank` at his position. */
function withOpinion(id: string, name: string, boardRank: number, csvRank: number) {
  return player(id, name, "WR", {
    position_rank: boardRank,
    second_opinion: { positional_rank: csvRank, overall_rank: csvRank * 3, source: "Clay" },
  });
}

function board(players: AvailablePlayer[], loadedAt: number | null = 1756425600) {
  render(
    <Board
      players={players}
      positions={["WR"]}
      loading={false}
      boardSize={players.length}
      secondOpinionLoadedAt={loadedAt}
      onDraft={vi.fn()}
    />,
  );
}

describe("the second-opinion column", () => {
  it("is absent entirely when nothing has been imported", () => {
    board([player("w1", "Alpha Wideout", "WR")], null);
    expect(screen.queryByRole("button", { name: /^Clay,/ })).toBeNull();
    // And no stand-in dash where the column would have been.
    expect(screen.queryByText("–")).toBeNull();
  });

  it("heads itself with the source and names the import date in its tooltip", () => {
    board([withOpinion("w1", "Alpha Wideout", 21, 9)]);
    const head = screen.getByRole("button", { name: /^Clay,/ });
    expect(head).toHaveAttribute("title", expect.stringContaining("Clay's rank at the position"));
    expect(head.getAttribute("title")).toContain(new Date(1756425600 * 1000).toLocaleDateString());
  });

  it("shows the source's positional rank against this board's", () => {
    board([withOpinion("w1", "Alpha Wideout", 21, 9)]);
    expect(screen.getByText("WR9")).toBeInTheDocument();
  });

  it("tints a big disagreement and says which way it runs", () => {
    board([
      withOpinion("w1", "Loved Wideout", 21, 9),
      withOpinion("w2", "Doubted Wideout", 4, 19),
      withOpinion("w3", "Agreed Wideout", 12, 14),
    ]);
    const loved = screen.getByText("WR9");
    expect(loved).toHaveClass("so-higher");
    expect(loved).toHaveAttribute("title", "Clay has him WR9; this board has him WR21");

    const doubted = screen.getByText("WR19");
    expect(doubted).toHaveClass("so-lower");
    expect(doubted).toHaveAttribute("title", "Clay has him WR19; this board has him WR4");

    // Two spots apart is not a disagreement worth a colour.
    const agreed = screen.getByText("WR14");
    expect(agreed).not.toHaveClass("so-higher");
    expect(agreed).not.toHaveClass("so-lower");
    expect(agreed).not.toHaveAttribute("title");
  });

  it("leaves a dash for a player the import did not match", () => {
    board([withOpinion("w1", "Known Wideout", 21, 9), player("w2", "Unknown Wideout", "WR")]);
    expect(screen.getByText("–")).toBeInTheDocument();
  });

  it("sorts by the imported rank, unmatched players last", () => {
    board([
      withOpinion("w1", "Third Wideout", 21, 30),
      player("w2", "Unmatched Wideout", "WR"),
      withOpinion("w3", "First Wideout", 21, 2),
    ]);
    fireEvent.click(screen.getByRole("button", { name: /^Clay,/ }));
    const names = screen
      .getAllByRole("button", { name: "Draft" })
      .map((b) => b.closest(".board-row")?.querySelector(".board-player")?.textContent ?? "");
    expect(names[0]).toContain("First Wideout");
    expect(names[1]).toContain("Third Wideout");
    expect(names[2]).toContain("Unmatched Wideout");
  });
});
