// The picker's list, which is two sources with an overlap between them.

import { describe, expect, it } from "vitest";
import { leagueNote, mergeLeagues } from "./leagues";
import type { StoredLeague } from "./types";

const league = (id: string, name: string, season = "2026"): StoredLeague => ({
  league_id: id,
  name,
  season,
});

describe("mergeLeagues", () => {
  it("puts the league on screen first, whichever list it came from", () => {
    const merged = mergeLeagues([league("1", "Alpha")], [league("2", "Beta")], "2");
    expect(merged.map((l) => l.league_id)).toEqual(["2", "1"]);
  });

  it("names a league once, with what Sleeper calls it now", () => {
    const merged = mergeLeagues([league("1", "Old name")], [league("1", "New name")], null);
    expect(merged).toHaveLength(1);
    expect(merged[0]?.name).toBe("New name");
  });

  it("reads newest season first, then alphabetically inside a season", () => {
    const merged = mergeLeagues(
      [league("1", "Zeta", "2026"), league("2", "Alpha", "2025"), league("3", "Alpha", "2026")],
      [],
      null,
    );
    expect(merged.map((l) => l.name)).toEqual(["Alpha", "Zeta", "Alpha"]);
    expect(merged.map((l) => l.season)).toEqual(["2026", "2026", "2025"]);
  });

  it("keeps a league the app has loaded that Sleeper does not list", () => {
    // A mock draft: loaded by ID, and never part of any account's leagues.
    const merged = mergeLeagues([league("mock", "Mock draft", "")], [league("1", "Real")], null);
    expect(merged.map((l) => l.league_id).sort()).toEqual(["1", "mock"]);
  });
});

describe("leagueNote", () => {
  it("says which one is on screen", () => {
    expect(leagueNote(league("1", "Alpha"), "1")).toBe("2026 season · on screen now");
    expect(leagueNote(league("1", "Alpha"), "2")).toBe("2026 season");
  });

  it("does not claim a season it was never told", () => {
    expect(leagueNote(league("1", "Alpha", ""), null)).toBe("season unknown");
  });
});
