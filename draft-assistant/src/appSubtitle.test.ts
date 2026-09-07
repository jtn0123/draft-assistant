import { describe, expect, it } from "vitest";
import { footerNote, headerMeta, headerSubtitle } from "./appSubtitle";
import type { StandingsRow } from "./season-types";
import { draftFixture, seasonFixture } from "./test/appHarness";

/** One standings row, named and seeded by the test and dull everywhere else. */
function standing(overrides: Partial<StandingsRow> = {}): StandingsRow {
  return {
    roster_id: 9,
    seed: 9,
    name: "Somebody",
    record: "0-0",
    wins: 0,
    losses: 0,
    ties: 0,
    points_for: 0,
    projected_points: 0,
    playoff_odds: 0.5,
    is_mine: false,
    ...overrides,
  };
}

/** Three teams, the user third by seed. Written out rather than derived: the
 *  point of these tests is the literal line the header shows. */
const threeTeams = (mine: boolean): StandingsRow[] => [
  standing({ roster_id: 2, seed: 1, name: "punt_god", record: "3-0" }),
  standing({ roster_id: 3, seed: 2, name: "Hurts Donut", record: "2-1" }),
  standing({ roster_id: 1, seed: 3, name: "You", record: "2-1", is_mine: mine }),
];

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
    const season = seasonFixture({ week: 5, standings: threeTeams(true) });
    expect(headerSubtitle("season", draftFixture(), season)).toBe("Week 5 · 2-1 · 3rd of 3");
  });

  it("counts the league instead when no row is the user's", () => {
    const season = seasonFixture({ week: 5, standings: threeTeams(false) });
    expect(headerSubtitle("season", draftFixture(), season)).toBe("Week 5 · 3 teams");
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
