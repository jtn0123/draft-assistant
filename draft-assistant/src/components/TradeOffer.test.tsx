import { render, screen } from "@testing-library/react";
import userEvent from "@testing-library/user-event";
import { beforeEach, describe, expect, it, vi } from "vitest";
import fixtureJson from "../../public/dev-fixture.json";
import type { DraftView } from "../types";

const testState = vi.hoisted(() => ({ api: { evaluateTrade: vi.fn() } }));
vi.mock("../api", () => ({ api: testState.api }));

import { TradeOffer } from "./TradeOffer";

function view(): DraftView {
  const v = structuredClone(fixtureJson) as unknown as DraftView;
  v.draft.my_slot = 2;
  v.draft.status = "complete";
  return v;
}

describe("TradeOffer", () => {
  beforeEach(() => vi.clearAllMocks());

  it("sends the ticked players to the partner chosen and shows both sides", async () => {
    const user = userEvent.setup();
    const v = view();
    const me = v.rosters.find((r) => r.slot === 2)!;
    const them = v.rosters.find((r) => r.slot !== 2 && r.players.length > 0)!;
    testState.api.evaluateTrade.mockResolvedValue({
      partner_slot: them.slot,
      partner_name: them.display_name,
      give: [],
      get: [],
      my_season_before: 1800,
      my_season_after: 1830,
      their_season_before: 1900,
      their_season_after: 1890,
      week: 1,
      my_week_before: 120,
      my_week_after: 123.4,
      their_week_before: 130,
      give_picks: [],
      get_picks: [],
      their_week_after: 129,
    });
    render(<TradeOffer view={v} />);
    await user.click(screen.getByLabelText(new RegExp(me.players[0].name)));
    await user.click(screen.getByLabelText(new RegExp(them.players[0].name)));
    await user.click(screen.getByRole("button", { name: "Price it" }));
    expect(testState.api.evaluateTrade).toHaveBeenCalledWith(
      them.slot,
      [me.players[0].player_id],
      [them.players[0].player_id],
      [],
      [],
    );
    const out = await screen.findByRole("status");
    expect(out).toHaveTextContent("Me +30");
    expect(out).toHaveTextContent(`${them.display_name} −10`);
    expect(out).toHaveTextContent("They lose on it");
  });

  it("trades a draft pick, prices it, and counts it in the verdict", async () => {
    // 34 of this league's 38 trades last season moved a pick: an offer that
    // cannot include one is priced in the wrong money (src-tauri pick_value).
    const user = userEvent.setup();
    const v = view();
    v.pick_prices = [
      { round: 1, points: 139, example: "Jaxon Smith-Njigba" },
      { round: 3, points: 56, example: "Emeka Egbuka" },
    ];
    const them = v.rosters.find((r) => r.slot !== 2 && r.players.length > 0)!;
    testState.api.evaluateTrade.mockResolvedValue({
      partner_slot: them.slot,
      partner_name: them.display_name,
      give: [],
      get: [],
      my_season_before: 1800,
      my_season_after: 1800,
      their_season_before: 1900,
      their_season_after: 1900,
      week: 1,
      my_week_before: 120,
      my_week_after: 120,
      their_week_before: 130,
      their_week_after: 130,
      give_picks: [{ round: 3, points: 56, example: "Emeka Egbuka" }],
      get_picks: [{ round: 1, points: 139, example: "Jaxon Smith-Njigba" }],
    });
    render(<TradeOffer view={v} />);
    const round = (side: string, r: number) =>
      screen.getAllByLabelText(new RegExp(`Round ${r} pick`))[side === "give" ? 0 : 1]!;
    await user.click(round("give", 3));
    await user.click(round("get", 1));
    expect(round("give", 3)).toHaveAttribute("aria-pressed", "true");
    await user.click(screen.getByRole("button", { name: "Price it" }));
    expect(testState.api.evaluateTrade).toHaveBeenCalledWith(them.slot, [], [], [3], [1]);
    const out = await screen.findByRole("status");
    // A first for a third, nothing else moving: +83 my way.
    expect(out).toHaveTextContent("Me +83");
    expect(out).toHaveTextContent(`${them.display_name} −83`);
    expect(out).toHaveTextContent("R1 in (+139) · R3 out (−56)");
    expect(out).toHaveTextContent("they pay next season");
  });

  it("shows the failure instead of a verdict", async () => {
    const user = userEvent.setup();
    const v = view();
    const me = v.rosters.find((r) => r.slot === 2)!;
    testState.api.evaluateTrade.mockRejectedValue(new Error("Pricing an offer needs the desktop app."));
    render(<TradeOffer view={v} />);
    await user.click(screen.getByLabelText(new RegExp(me.players[0].name)));
    await user.click(screen.getByRole("button", { name: "Price it" }));
    expect(await screen.findByText(/needs the desktop app/)).toBeInTheDocument();
  });
});
