import { describe, expect, it } from "vitest";
import type { AvailablePlayer, DraftView } from "./types";
import { sameAvailable, stableAvailable } from "./boardIdentity";

function player(over: Partial<AvailablePlayer> = {}): AvailablePlayer {
  return {
    player_id: "a",
    name: "Alpha",
    position: "RB",
    team: "SF",
    bye_week: 9,
    points: 210,
    bonus_points: 4,
    vorp: 40,
    tier: 1,
    position_rank: 1,
    overall_rank: 1,
    adp: 3.5,
    injury_status: null,
    sleeper_pts_ppr: 208,
    survival_next: 0.42,
    ...over,
  };
}

const view = (available: AvailablePlayer[]): DraftView => ({ available }) as DraftView;

describe("sameAvailable", () => {
  it("holds for the same array and for a deep copy of it", () => {
    const a = [player(), player({ player_id: "b", name: "Bravo" })];
    expect(sameAvailable(a, a)).toBe(true);
    expect(
      sameAvailable(
        a,
        a.map((p) => ({ ...p })),
      ),
    ).toBe(true);
  });

  it("fails on a different length", () => {
    expect(sameAvailable([player()], [])).toBe(false);
    expect(sameAvailable([], [player()])).toBe(false);
  });

  it("fails on a different order of the same players", () => {
    const a = player();
    const b = player({ player_id: "b", name: "Bravo" });
    expect(sameAvailable([a, b], [b, a])).toBe(false);
  });

  // Every field, one at a time: a miss here is a board showing stale numbers.
  it.each([
    ["player_id", { player_id: "z" }],
    ["name", { name: "Zulu" }],
    ["position", { position: "WR" }],
    ["team", { team: null }],
    ["bye_week", { bye_week: 10 }],
    ["points", { points: 211 }],
    ["bonus_points", { bonus_points: 5 }],
    ["vorp", { vorp: 41 }],
    ["tier", { tier: 2 }],
    ["position_rank", { position_rank: 2 }],
    ["overall_rank", { overall_rank: 2 }],
    ["adp", { adp: null }],
    ["injury_status", { injury_status: "Out" }],
    ["sleeper_pts_ppr", { sleeper_pts_ppr: null }],
    ["survival_next", { survival_next: 0.43 }],
  ] satisfies [string, Partial<AvailablePlayer>][])("notices a changed %s", (_field, change) => {
    expect(sameAvailable([player()], [player(change)])).toBe(false);
  });
});

describe("stableAvailable", () => {
  it("returns the new view untouched when there is no previous one", () => {
    const next = view([player()]);
    expect(stableAvailable(null, next)).toBe(next);
  });

  it("recycles the previous pool but keeps everything else from the new view", () => {
    const pool = [player()];
    const prev = { ...view(pool), generated_at: 100, schema_version: "1" };
    const next = { ...view([player()]), generated_at: 200, schema_version: "2" };

    const merged = stableAvailable(prev, next);
    expect(merged).not.toBe(prev);
    expect(merged.available).toBe(pool);
    expect(merged.generated_at).toBe(200);
    expect(merged.schema_version).toBe("2");
  });

  it("keeps the new pool when a projection moved", () => {
    const prev = view([player()]);
    const next = view([player({ points: 188 })]);
    expect(stableAvailable(prev, next)).toBe(next);
    expect(stableAvailable(prev, next).available[0].points).toBe(188);
  });
});
