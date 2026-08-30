import { render, screen } from "@testing-library/react";
import userEvent from "@testing-library/user-event";
import { beforeEach, describe, expect, it, vi } from "vitest";

const testState = vi.hoisted(() => ({ api: { evaluateTrade: vi.fn() } }));
vi.mock("../api", () => ({ api: testState.api }));
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
      { at: 1787968391346, week: 1, kind: "free_agent", status: "complete", teams: ["youngmomo"], adds: [["youngmomo", "Cooper Kupp"]], drops: [], picks: [], bid: null },
      { at: 1787103066111, week: 1, kind: "trade", status: "complete", teams: ["fisher23", "ocrevo"], adds: [], drops: [], picks: ["2026 round 4 (ocrevo) → fisher23"], bid: null },
      { at: 1787066398713, week: 1, kind: "waiver", status: "complete", teams: ["ChrisWitz"], adds: [["ChrisWitz", "Jonnu Smith"]], drops: [["ChrisWitz", "Someone Else"]], picks: [], bid: 37 },
    ];
    v.trade_ideas = [
      { partner_slot: 8, partner_name: "fisher23", give_id: "a", give: "Carnell Tate", give_position: "WR", also_give_id: null, also_give: null, also_give_position: null, get_id: "b", get: "Tony Pollard", get_position: "RB", my_gain: 21.4, over_waiver: 18.0, their_gain: 9.2, partner_trades: 7 },
      { partner_slot: 13, partner_name: "youngmomo", give_id: "c", give: "Khalil Shakir", give_position: "WR", also_give_id: "d", also_give: "Xavier Worthy", also_give_position: "WR", get_id: "e", get: "Chase Brown", get_position: "RB", my_gain: 30, over_waiver: 26, their_gain: 12, partner_trades: 7 },
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

describe("Activity prices a trade idea in one tap", () => {
  beforeEach(() => vi.clearAllMocks());

  it("fills the offer from the idea, both pieces of a two-for-one, and prices it", async () => {
    const user = userEvent.setup();
    const v = structuredClone(fixtureJson) as unknown as DraftView;
    v.draft.my_slot = 2;
    v.draft.status = "complete";
    const them = v.rosters.find((r) => r.slot !== 2 && r.players.length > 0)!;
    v.trade_ideas = [
      {
        partner_slot: them.slot,
        partner_name: them.display_name,
        give_id: "g1",
        give: "Khalil Shakir",
        give_position: "WR",
        also_give_id: "g2",
        also_give: "Xavier Worthy",
        also_give_position: "WR",
        get_id: "t1",
        get: "Chase Brown",
        get_position: "RB",
        my_gain: 20,
        over_waiver: 18,
        their_gain: 9, partner_trades: 7,
      },
    ];
    testState.api.evaluateTrade.mockResolvedValue({
      partner_slot: them.slot,
      partner_name: them.display_name,
      give: [],
      get: [],
      my_season_before: 1800,
      my_season_after: 1818,
      their_season_before: 1900,
      their_season_after: 1909,
      week: 1,
      my_week_before: 120,
      my_week_after: 121,
      their_week_before: 130,
      give_picks: [],
      get_picks: [],
      their_week_after: 131,
    });
    render(<Activity view={v} />);
    await user.click(screen.getByRole("button", { name: "Price Chase Brown for Khalil Shakir" }));
    expect(testState.api.evaluateTrade).toHaveBeenCalledWith(them.slot, ["g1", "g2"], ["t1"], [], []);
    const out = await screen.findByRole("status");
    expect(out).toHaveTextContent("Me +18");
    expect(out).toHaveTextContent("Both sides gain");
    // The form opened on the same partner, so the offer can be adjusted from here.
    expect(screen.getByRole("combobox", { name: "Trade partner" })).toHaveValue(String(them.slot));
  });
});

describe("Activity: lost claims and who trades", () => {
  it("shows a lost claim greyed with the bid, and the partner's trade habit on an idea", () => {
    const v = structuredClone(fixtureJson) as unknown as DraftView;
    v.draft.my_slot = 2;
    v.draft.status = "complete";
    v.activity = [
      { at: 1787968391346, week: 1, kind: "waiver", status: "failed", teams: ["skoLinNH"], adds: [["skoLinNH", "Harold Fannin"]], drops: [], picks: [], bid: 41 },
    ];
    v.trade_ideas = [
      { partner_slot: 8, partner_name: "fisher23", give_id: "a", give: "Carnell Tate", give_position: "WR", also_give_id: null, also_give: null, also_give_position: null, get_id: "b", get: "Tony Pollard", get_position: "RB", my_gain: 21.4, over_waiver: 18.0, their_gain: 9.2, partner_trades: 0 },
      { partner_slot: 13, partner_name: "youngmomo", give_id: "c", give: "Khalil Shakir", give_position: "WR", also_give_id: null, also_give: null, also_give_position: null, get_id: "e", get: "Chase Brown", get_position: "RB", my_gain: 30, over_waiver: 26, their_gain: 12, partner_trades: null },
    ];
    render(<Activity view={v} />);
    const lost = screen.getByRole("list", { name: "League activity" }).querySelector("li")!;
    expect(lost).toHaveClass("failed");
    expect(lost).toHaveTextContent("skoLinNH bid ($41) on Harold Fannin — lost");
    const ideas = screen.getByRole("list", { name: "Trade ideas" });
    expect(ideas).toHaveTextContent("fisher23 · never traded");
    // No history for a manager new to the league: say nothing rather than "0".
    expect(ideas).not.toHaveTextContent("youngmomo ·");
  });
});
