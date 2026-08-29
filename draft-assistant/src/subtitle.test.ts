import { describe, expect, it } from "vitest";
import fixtureJson from "../public/dev-fixture.json";
import type { DraftView } from "./types";
import { subtitle } from "./subtitle";

function view(): DraftView {
  return structuredClone(fixtureJson) as unknown as DraftView;
}

describe("subtitle", () => {
  it("describes the draft on the draft screen, and until there is a week", () => {
    const v = view();
    expect(subtitle(v, "draft")).toBe("2026 · 14 teams · 15 rounds · manual picks active");
    v.draft.manual_picks_active = false;
    expect(subtitle(v, "draft")).toBe("2026 · 14 teams · 15 rounds");
    v.this_week = null;
    expect(subtitle(v, "season")).toBe("2026 · 14 teams · 15 rounds");
  });

  it("says where the season is on the season screen", () => {
    const v = view();
    v.this_week = {
      week: 3,
      lineup: null,
      matchup: {
        opponent_slot: 6,
        opponent_name: "MeatballMike09",
        my_points: 120,
        opponent_points: 118,
        margin: 2,
        win_probability: 0.53,
        my_starters: [],
        opponent_starters: [],
      },
    };
    expect(subtitle(v, "season")).toBe("2026 · Week 3 · vs MeatballMike09");
    v.season = {
      through_week: 2,
      standings: [
        { slot: v.draft.my_slot!, display_name: "me", wins: 2, losses: 0, ties: 0, points_for: 250, points_against: 200 },
      ],
      my_results: [],
      trends: [],
    };
    expect(subtitle(v, "season")).toBe("2026 · Week 3 · 2–0 · vs MeatballMike09");
  });
});
