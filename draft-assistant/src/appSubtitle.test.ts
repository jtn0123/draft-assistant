import { describe, expect, it } from "vitest";
import { headerSubtitle } from "./appSubtitle";
import { ordinal } from "./format";
import { draftFixture, seasonFixture } from "./test/appHarness";

describe("the header's second line", () => {
  it("counts the draft's progress on the board", () => {
    const view = draftFixture();
    view.draft.current_round = 3;
    view.draft.rounds = 15;
    view.draft.total_picks_made = 41;
    expect(headerSubtitle("draft", view, null)).toBe("Round 3 of 15 · 41 picks in");
  });

  it("names the season's year until the week has loaded", () => {
    const view = draftFixture();
    expect(headerSubtitle("season", view, null)).toBe(`${view.league.season} season`);
  });

  it("names the week and the user's record once it has", () => {
    const season = seasonFixture();
    const mine = season.standings.find((s) => s.is_mine);
    expect(headerSubtitle("season", draftFixture(), season)).toBe(
      `Week ${season.week} · ${mine?.record} · ${ordinal(mine?.seed ?? 0)} of ${season.standings.length}`,
    );
  });
});
