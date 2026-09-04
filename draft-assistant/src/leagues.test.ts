// The picker's list, which is two sources with an overlap between them.

import { describe, expect, it } from "vitest";
import { leagueNote, leagueStage, mergeLeagues, platformMark, platformOf } from "./leagues";
import type { Platform, StoredLeague } from "./types";

const league = (
  id: string,
  name: string,
  season = "2026",
  status: string | null = null,
  platform: Platform = "sleeper",
): StoredLeague => ({
  league_id: id,
  name,
  season,
  status,
  platform,
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

  it("keeps both accounts' answers in one list", () => {
    const merged = mergeLeagues(
      [league("1", "Alpha")],
      [league("2", "Beta"), league("449.l.9", "Gamma", "2026", null, "yahoo")],
      null,
    );
    expect(merged.map((l) => l.league_id).sort()).toEqual(["1", "2", "449.l.9"]);
    expect(merged.find((l) => l.league_id === "449.l.9")?.platform).toBe("yahoo");
  });

  it("will not let one platform's answer relabel the other's league", () => {
    // Ids never actually collide across the two services, so this is a
    // guard rather than a case: whatever went wrong, the stored row stands.
    const merged = mergeLeagues(
      [league("1", "Kept", "2026", null, "yahoo")],
      [league("1", "Overwritten")],
      null,
    );
    expect(merged).toHaveLength(1);
    expect(merged[0]?.name).toBe("Kept");
    expect(merged[0]?.platform).toBe("yahoo");
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

  it("says where the league is when Sleeper has said", () => {
    expect(leagueNote(league("1", "Sharks", "2026", "pre_draft"), null)).toBe(
      "2026 season · draft ahead",
    );
    expect(leagueNote(league("1", "Sharks", "2026", "drafting"), "1")).toBe(
      "2026 season · drafting now · on screen now",
    );
  });
});

describe("leagueStage", () => {
  it("translates each of Sleeper's words, and stays quiet on anything else", () => {
    expect(leagueStage("in_season")).toBe("in season");
    expect(leagueStage("complete")).toBe("finished");
    expect(leagueStage(null)).toBeNull();
    expect(leagueStage("something_new")).toBeNull();
  });
});

describe("platformMark", () => {
  it("marks a Yahoo league and leaves the ordinary case unmarked", () => {
    expect(platformMark("yahoo")).toBe("Yahoo");
    expect(platformMark("sleeper")).toBeNull();
  });
});

describe("platformOf", () => {
  it("reads the service off the shape of a pasted id", () => {
    expect(platformOf("449.l.12345")).toBe("yahoo");
    expect(platformOf("1389710366300200960")).toBe("sleeper");
    // A mock draft's id is neither a Yahoo key nor anything else special.
    expect(platformOf("mock")).toBe("sleeper");
  });
});
