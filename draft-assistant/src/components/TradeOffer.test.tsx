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
      their_week_after: 129,
    });
    render(<TradeOffer view={v} />);
    await user.click(screen.getByLabelText(new RegExp(me.players[0].name)));
    await user.click(screen.getByLabelText(new RegExp(them.players[0].name)));
    await user.click(screen.getByRole("button", { name: "Price it" }));
    expect(testState.api.evaluateTrade).toHaveBeenCalledWith(them.slot, [me.players[0].player_id], [them.players[0].player_id]);
    const out = await screen.findByRole("status");
    expect(out).toHaveTextContent("Me +30");
    expect(out).toHaveTextContent(`${them.display_name} −10`);
    expect(out).toHaveTextContent("They lose on it");
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
