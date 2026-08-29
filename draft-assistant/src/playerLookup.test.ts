import { describe, expect, it } from "vitest";
import fixtureJson from "../public/dev-fixture.json";
import type { DraftView, Starter } from "./types";
import { playerFacts } from "./playerLookup";

function view(): DraftView {
  return structuredClone(fixtureJson) as unknown as DraftView;
}
const st = (slot: string, id: string, name: string, points: number, injury: string | null = null): Starter => ({
  slot,
  player_id: id,
  name,
  position: slot,
  points,
  injury,
});

describe("playerFacts", () => {
  it("knows nothing about an id that is nowhere", () => {
    expect(playerFacts(view(), "nobody")).toBeNull();
  });

  it("gathers a rostered starter from the rosters, the standings and the week", () => {
    const v = view();
    const me = v.rosters.find((r) => r.slot === v.draft.my_slot)!;
    const p = me.players[0];
    v.projected_standings = [
      {
        slot: me.slot,
        display_name: "me",
        full_strength: 1900,
        season: 1850,
        starters: [st("RB", p.player_id, p.name, 245)],
        week: 1,
        week_points: 120,
        week_starters: [st("RB", p.player_id, p.name, 14.2, "Questionable")],
      },
    ];
    v.bye_weeks = [{ week: 7, out: [p.name], points: 100, shortfall: 12, empty_slots: [] }];
    const f = playerFacts(v, p.player_id)!;
    expect(f.name).toBe(p.name);
    expect(f.owner).toBe("YOU");
    expect(f.round).toBe(p.round);
    expect(f.season).toBe(245);
    expect(f.week).toBe(14.2);
    expect(f.weekNo).toBe(1);
    expect(f.injury).toBe("Questionable");
    expect(f.bye).toBe(7);
  });

  it("names another manager as the owner, and a waiver target as a free agent", () => {
    const v = view();
    const them = v.rosters.find((r) => r.slot !== v.draft.my_slot && r.players.length > 0)!;
    expect(playerFacts(v, them.players[0].player_id)!.owner).toBe(them.display_name);

    v.waivers = {
      targets: [
        {
          player_id: "fa1",
          name: "New York Giants",
          position: "DEF",
          team: "NYG",
          bye_week: 14,
          points: 95,
          my_gain: 92,
          rivals_helped: 1,
          trending_adds: 4000,
          suggested_bid: 10,
        },
      ],
      drops: [],
    };
    const fa = playerFacts(v, "fa1")!;
    expect(fa.owner).toBeNull();
    expect(fa.season).toBe(95);
    expect(fa.bye).toBe(14);
    expect(fa.trendingAdds).toBe(4000);
  });

  it("reads a free agent off the board with his ADP", () => {
    const v = view();
    const b = v.available[0];
    const f = playerFacts(v, b.player_id)!;
    expect(f.name).toBe(b.name);
    expect(f.owner).toBeNull();
    expect(f.season).toBe(b.points);
    expect(f.adp).toBe(b.adp);
  });
});
