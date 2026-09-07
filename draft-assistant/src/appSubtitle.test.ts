import { describe, expect, it } from "vitest";
import { footerNote, headerMeta, headerSubtitle } from "./appSubtitle";
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

describe("the header's third line", () => {
  it("names the format, the size and the round count", () => {
    const view = draftFixture();
    view.draft.teams = 12;
    view.draft.rounds = 15;
    view.draft.manual_picks_active = false;
    view.league.scoring_settings = { rec: 0.5 };
    expect(headerMeta(view)).toBe("12-team half-PPR · 15 rounds");
  });

  it("says so while picks are being typed in by hand", () => {
    const view = draftFixture();
    view.draft.manual_picks_active = true;
    expect(headerMeta(view)).toMatch(/ · manual picks active$/);
  });
});

describe("the line under the settings menu", () => {
  it("names the league and its id on Sleeper", () => {
    const view = draftFixture();
    view.league.platform = "sleeper";
    expect(footerNote(view)).toBe(
      `${view.league.name} · league ${view.league.league_id} · read-only connection`,
    );
  });

  it("carries Yahoo's attribution on a Yahoo league", () => {
    const view = draftFixture();
    view.league.platform = "yahoo";
    expect(footerNote(view)).toBe("Fantasy data provided by Yahoo Fantasy · read-only connection");
  });
});
