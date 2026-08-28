import { render, screen, within } from "@testing-library/react";
import { describe, expect, it } from "vitest";
import fixtureJson from "../../public/dev-fixture.json";
import type { DraftView } from "../types";
import { SnakeStrip } from "./SnakeStrip";

function view(): DraftView {
  return structuredClone(fixtureJson) as unknown as DraftView;
}

/** A 14-team snake mid-round-one, with the given pick on the clock. */
function live(pick: number, mySlot: number, myNext: number[]): DraftView {
  const v = view();
  v.draft.status = "drafting";
  v.draft.teams = 14;
  v.draft.rounds = 15;
  v.draft.current_pick = pick;
  v.draft.current_round = Math.floor((pick - 1) / 14) + 1;
  const index = (pick - 1) % 14;
  const round = Math.floor((pick - 1) / 14) + 1;
  v.draft.on_clock_slot = round % 2 === 1 ? index + 1 : 14 - index;
  v.draft.my_slot = mySlot;
  v.draft.my_next_picks = myNext;
  v.draft.is_my_pick = v.draft.on_clock_slot === mySlot;
  v.draft.picks_until_mine = myNext[0] - pick;
  v.draft.pick_deadline = null;
  v.rosters = Array.from({ length: 14 }, (_, i) => ({
    slot: i + 1,
    display_name: `Team${i + 1}`,
    players: [],
    open_starters: [],
  }));
  return v;
}

const cells = () => screen.getAllByRole("listitem");

describe("SnakeStrip", () => {
  it("draws every pick from the clock up to yours, and says how many are ahead", () => {
    render(<SnakeStrip view={live(3, 8, [8, 21])} />);
    // Picks 3..8 inclusive: five managers, then you.
    expect(cells()).toHaveLength(6);
    expect(cells()[0]).toHaveClass("on-clock");
    expect(cells()[0]).toHaveTextContent("Team3");
    const last = cells()[5];
    expect(last).toHaveClass("mine");
    expect(last).toHaveTextContent("YOU");
    expect(screen.getByText("5 ahead of you")).toBeInTheDocument();
  });

  it("reverses on even rounds, because that is what a snake does", () => {
    // Pick 15 opens round 2, which runs back down from slot 14.
    render(<SnakeStrip view={live(15, 12, [17])} />);
    expect(cells()[0]).toHaveTextContent("Team14");
    expect(cells()[1]).toHaveTextContent("Team13");
    // Slot 12 is mine on the way back down, and mine is always drawn as YOU.
    expect(cells()[2]).toHaveClass("mine");
    expect(cells()[2]).toHaveTextContent("YOU");
  });

  it("strikes through picks already in the book and still counts the live ones", () => {
    const v = live(3, 8, [8]);
    // Two keepers sit at picks 5 and 6: in the order, but nobody waits.
    v.rosters[4].players = [{ player_id: "k1", name: "Kept One", position: "RB", team: null, pick_no: 5, round: 1, is_keeper: true }];
    v.rosters[5].players = [{ player_id: "k2", name: "Kept Two", position: "WR", team: null, pick_no: 6, round: 1, is_keeper: true }];
    v.draft.picks_until_mine = 3;
    render(<SnakeStrip view={v} />);
    const kept = cells().filter((c) => c.className.includes("kept"));
    expect(kept).toHaveLength(2);
    expect(within(kept[0]).getByText("Team5")).toBeInTheDocument();
    expect(screen.getByText("3 ahead of you")).toBeInTheDocument();
  });

  it("skips the middle and still ends on you when your pick is far off", () => {
    // Pick 3 with yours at 27 is 25 cells; the strip keeps the near stretch,
    // marks the gap, and finishes on you rather than running off the end.
    render(<SnakeStrip view={live(3, 2, [27])} />);
    const list = cells().filter((c) => c.className.includes("snake-cell"));
    expect(list.length).toBeLessThanOrEqual(16);
    expect(list[0]).toHaveTextContent("Team3");
    expect(screen.getByLabelText("10 more picks")).toHaveTextContent("+10");
    const last = list[list.length - 1];
    expect(last).toHaveClass("mine");
    expect(last).toHaveTextContent("27");
    expect(last).toHaveTextContent("YOU");
  });

  it("celebrates instead of counting when it is your pick", () => {
    render(<SnakeStrip view={live(8, 8, [8, 21])} />);
    expect(screen.getByText("it's you 🎉")).toBeInTheDocument();
    expect(cells()[0]).toHaveClass("mine", "on-clock");
  });

  it("says nothing before the draft starts, when it is over, or if the order does not add up", () => {
    const pre = live(1, 8, [2, 27]);
    pre.draft.status = "pre_draft";
    pre.draft.is_my_pick = false;
    render(<SnakeStrip view={pre} />);
    expect(screen.getByText("waiting to start")).toBeInTheDocument();
    expect(screen.queryByText(/ahead of you/)).toBeNull();

    const done = live(3, 8, [8]);
    done.draft.status = "complete";
    const { container: over } = render(<SnakeStrip view={done} />);
    expect(over).toBeEmptyDOMElement();

    // A draft type this does not model: name nobody rather than name wrongly.
    const odd = live(3, 8, [8]);
    odd.draft.on_clock_slot = 9;
    const { container: unknown } = render(<SnakeStrip view={odd} />);
    expect(unknown).toBeEmptyDOMElement();
  });
});
