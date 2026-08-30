import { render, screen } from "@testing-library/react";
import userEvent from "@testing-library/user-event";
import { describe, expect, it, vi } from "vitest";
import type { AvailablePlayer } from "../types";
import { Board } from "./Board";

function player(id: string, name: string, position: string): AvailablePlayer {
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
  };
}

describe("Board", () => {
  it("builds position filters from league data, including kicker", async () => {
    const user = userEvent.setup();
    render(
      <Board
        players={[
          player("qb", "Quarterback", "QB"),
          player("k", "Kicker", "K"),
        ]}
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
