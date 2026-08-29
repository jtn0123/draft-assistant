import { render, screen, within } from "@testing-library/react";
import { describe, expect, it } from "vitest";
import fixtureJson from "../../public/dev-fixture.json";
import type { AvailablePlayer, DraftView } from "../types";
import { AtRisk } from "./AtRisk";
import { riskRanked } from "./valueAtRisk";

function player(over: Partial<AvailablePlayer>): AvailablePlayer {
  return {
    player_id: over.name ?? "p",
    name: "Player",
    position: "RB",
    team: "AAA",
    bye_week: 7,
    points: 200,
    bonus_points: 0,
    vorp: 50,
    tier: 3,
    position_rank: 1,
    overall_rank: 1,
    adp: 20,
    injury_status: null,
    sleeper_pts_ppr: null,
    survival_next: 0.5,
    ...over,
  } as AvailablePlayer;
}

function view(available: AvailablePlayer[], currentPick = 10, next = [27, 30]): DraftView {
  const v = structuredClone(fixtureJson) as unknown as DraftView;
  v.available = available;
  v.draft.current_pick = currentPick;
  v.draft.my_next_picks = next;
  return v;
}

describe("Won't last", () => {
  it("ranks by the value you lose, not by who goes first", () => {
    // Value lost = VORP x the chance you lose it. The star who probably
    // goes (200 x 0.5 = 100) beats the scrub who certainly goes (10 x 0.9 = 9),
    // which beats the stud who will still be there (300 x 0.01 = 3).
    const doomed = player({ name: "Doomed Scrub", vorp: 10, survival_next: 0.1 });
    const star = player({ name: "Falling Star", vorp: 200, survival_next: 0.5 });
    const safe = player({ name: "Safe Stud", vorp: 300, survival_next: 0.99 });
    const ranked = riskRanked(view([doomed, star, safe]));
    expect(ranked.map((r) => r.player.name)).toEqual([
      "Falling Star",
      "Doomed Scrub",
      "Safe Stud",
    ]);
  });

  it("names the pick you are waiting for and shows the odds", () => {
    render(<AtRisk view={view([player({ name: "Falling Star", survival_next: 0.22 })])} />);
    expect(screen.getByRole("heading", { name: /Won't last to 27/ })).toBeInTheDocument();
    const list = screen.getByRole("list", { name: "Players unlikely to last" });
    expect(within(list).getByText("Falling Star")).toBeInTheDocument();
    expect(within(list).getByText("22%")).toHaveClass("surv", "low");
  });

  it("says nothing when there is no next pick or nothing is at risk", () => {
    const noPick = render(<AtRisk view={view([player({})], 10, [])} />);
    expect(noPick.container).toBeEmptyDOMElement();
    noPick.unmount();

    // Everyone certain to last, and players with no ADP signal at all.
    const settled = render(
      <AtRisk view={view([player({ survival_next: 1 }), player({ survival_next: null })])} />,
    );
    expect(settled.container).toBeEmptyDOMElement();
  });
});
