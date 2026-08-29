import { render, screen } from "@testing-library/react";
import { describe, expect, it } from "vitest";
import fixtureJson from "../../public/dev-fixture.json";
import type { DraftView } from "../types";
import { Activity } from "./Activity";

function view(): DraftView {
  return structuredClone(fixtureJson) as unknown as DraftView;
}

describe("Activity", () => {
  it("renders nothing with no moves and no ideas", () => {
    const v = view();
    v.activity = [];
    v.trade_ideas = [];
    const { container } = render(<Activity view={v} />);
    expect(container).toBeEmptyDOMElement();
  });

  it("describes claims, trades and drops, and lists swaps with both gains", () => {
    const v = view();
    v.activity = [
      { at: 1787968391346, week: 1, kind: "free_agent", teams: ["youngmomo"], adds: [["youngmomo", "Cooper Kupp"]], drops: [], picks: [], bid: null },
      { at: 1787103066111, week: 1, kind: "trade", teams: ["fisher23", "ocrevo"], adds: [], drops: [], picks: ["2026 round 4 (ocrevo) → fisher23"], bid: null },
      { at: 1787066398713, week: 1, kind: "waiver", teams: ["ChrisWitz"], adds: [["ChrisWitz", "Jonnu Smith"]], drops: [["ChrisWitz", "Someone Else"]], picks: [], bid: 37 },
    ];
    v.trade_ideas = [
      { partner_slot: 8, partner_name: "fisher23", give_id: "a", give: "Carnell Tate", give_position: "WR", also_give_id: null, also_give: null, also_give_position: null, get_id: "b", get: "Tony Pollard", get_position: "RB", my_gain: 21.4, over_waiver: 18.0, their_gain: 9.2 },
      { partner_slot: 13, partner_name: "youngmomo", give_id: "c", give: "Khalil Shakir", give_position: "WR", also_give_id: "d", also_give: "Xavier Worthy", also_give_position: "WR", get_id: "e", get: "Chase Brown", get_position: "RB", my_gain: 30, over_waiver: 26, their_gain: 12 },
    ];
    render(<Activity view={v} />);
    const ideas = screen.getByRole("list", { name: "Trade ideas" });
    expect(ideas).toHaveTextContent("Tony Pollard");
    expect(ideas).toHaveTextContent("for Carnell Tate");
    expect(ideas).toHaveTextContent("+18");
    expect(ideas).toHaveTextContent("them +9");
    expect(ideas).toHaveTextContent("Chase Brown RB for Khalil Shakir WR + Xavier Worthy WR");
    const moves = screen.getByRole("list", { name: "League activity" });
    expect(moves).toHaveTextContent("youngmomo added Cooper Kupp");
    expect(moves).toHaveTextContent("fisher23 ↔ ocrevo: 2026 round 4 (ocrevo) → fisher23");
    expect(moves).toHaveTextContent("ChrisWitz claimed Jonnu Smith ($37), dropped Someone Else");
  });
});
