import { describe, expect, it } from "vitest";
import {
  clockLabel,
  kickoffLabel,
  pickLabel,
  posRank,
  scoringFormat,
  signed,
  untilLabel,
  lockLabel,
} from "./format";

describe("pickLabel", () => {
  it("renders round.pick with a padded pick number", () => {
    expect(pickLabel(1, 12)).toBe("1.01");
    expect(pickLabel(12, 12)).toBe("1.12");
    expect(pickLabel(28, 12)).toBe("3.04");
  });

  it("degrades to the raw number when the team count is unknown", () => {
    expect(pickLabel(28, 0)).toBe("28");
  });
});

describe("signed", () => {
  it("uses a typographic minus, not a hyphen", () => {
    expect(signed(4)).toBe("+4.0");
    expect(signed(-3.75)).toBe("−3.8");
    expect(signed(0)).toBe("−0.0");
  });
});

describe("kickoffLabel", () => {
  it("names the window in Eastern time regardless of the local zone", () => {
    // 2025-09-07T17:00:00Z is Sunday 1:00pm ET.
    expect(kickoffLabel(Date.parse("2025-09-07T17:00:00Z"))).toBe("Sun 1:00 ET");
    // 2025-09-08T00:20:00Z is Sunday 8:20pm ET, not Monday.
    expect(kickoffLabel(Date.parse("2025-09-08T00:20:00Z"))).toBe("Sun 8:20 ET");
  });

  it("returns nothing for a missing kickoff", () => {
    expect(kickoffLabel(0)).toBe("");
  });
});

describe("untilLabel", () => {
  it("counts down in the largest two units", () => {
    // The label floors, so a deadline exactly N units away would tick down to
    // N-1 between capturing `now` and the call. Half a minute of slack keeps
    // the assertion on the intended side of the boundary.
    const from = (ms: number) => untilLabel(Date.now() + ms + 30_000);
    expect(from(54 * 3600_000)).toBe("2d 6h");
    expect(from(6 * 3600_000 + 12 * 60_000)).toBe("6h 12m");
    expect(from(48 * 60_000)).toBe("48m");
  });

  it("reads as now once the deadline has passed", () => {
    expect(untilLabel(Date.now() - 1000)).toBe("now");
    expect(untilLabel(null)).toBe("–");
  });
});

describe("lockLabel", () => {
  it("spells out the kickoff behind the countdown in Eastern time", () => {
    expect(lockLabel(Date.parse("2026-09-11T00:15:00Z"))).toBe("Thu Sep 10 · 8:15 PM ET");
    expect(lockLabel(null)).toBe("");
  });
});

describe("posRank", () => {
  it("appends the rank only when there is one", () => {
    expect(posRank("WR", 14)).toBe("WR14");
    expect(posRank("WR", null)).toBe("WR");
  });
});

describe("scoringFormat", () => {
  it("names the reception value the way the header does", () => {
    expect(scoringFormat(1)).toBe("full-PPR");
    expect(scoringFormat(0.5)).toBe("half-PPR");
    expect(scoringFormat(0)).toBe("standard");
    expect(scoringFormat(undefined)).toBe("standard");
  });
});

describe("clockLabel", () => {
  it("counts down in m:ss and clamps at zero", () => {
    expect(clockLabel(41_000, 0)).toBe("0:41");
    expect(clockLabel(90_000, 0)).toBe("1:30");
    expect(clockLabel(5_000, 9_000)).toBe("0:00");
    expect(clockLabel(null, 0)).toBeNull();
  });
});
