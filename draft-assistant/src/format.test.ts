import { afterEach, describe, expect, it, vi } from "vitest";
import {
  age,
  clockLabel,
  ideasAgeNote,
  injuryWord,
  kickoffLabel,
  nowSecs,
  ordinal,
  pickLabel,
  posRank,
  scoringFormat,
  signed,
  spanLabel,
  untilLabel,
  lockLabel,
} from "./format";

// Grade item D8. Everything below that reads the wall clock does so through a
// frozen one: a countdown that floors cannot be asserted against a clock that
// moves between capturing `now` and the call, and it is the exact rollover —
// not a point comfortably inside a bucket — that is worth pinning down.
const NOW = Date.parse("2026-09-13T17:00:00Z");

/** Freeze the clock at `NOW` for the duration of one test. */
function freeze(): void {
  vi.useFakeTimers();
  vi.setSystemTime(NOW);
}

afterEach(() => {
  vi.useRealTimers();
  vi.restoreAllMocks();
});

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

  it("falls back to the platform format when the Eastern one is unavailable", () => {
    // Some webviews ship without full ICU data. A window that cannot name the
    // kickoff window must still show a time rather than throwing through the
    // whole games list.
    vi.spyOn(Intl, "DateTimeFormat").mockImplementation(() => {
      throw new Error("no time zone data");
    });
    const ms = Date.parse("2025-09-07T17:00:00Z");
    expect(kickoffLabel(ms)).toBe(new Date(ms).toLocaleString());
  });
});

describe("untilLabel", () => {
  /** A deadline `ms` from the frozen now. */
  const from = (ms: number) => untilLabel(NOW + ms);

  it("counts down in the largest two units", () => {
    freeze();
    expect(from(54 * 3600_000)).toBe("2d 6h");
    expect(from(6 * 3600_000 + 12 * 60_000)).toBe("6h 12m");
    expect(from(48 * 60_000)).toBe("48m");
  });

  it("rolls over to a coarser unit at the exact millisecond, not before", () => {
    freeze();
    // A day appears the instant the deadline is 24h out, and the hour it
    // leaves behind is zero — never "1d 24h" or a skipped "2d 0h".
    expect(from(48 * 3600_000)).toBe("2d 0h");
    expect(from(48 * 3600_000 - 1)).toBe("1d 23h");
    expect(from(24 * 3600_000)).toBe("1d 0h");
    expect(from(24 * 3600_000 - 1)).toBe("23h 59m");
    // The same edge one unit down: minutes give way to hours only at the hour.
    expect(from(3600_000)).toBe("1h 0m");
    expect(from(3600_000 - 1)).toBe("59m");
    // And the smallest bucket: under a minute still reads as a countdown.
    expect(from(60_000)).toBe("1m");
    expect(from(60_000 - 1)).toBe("0m");
  });

  it("reads as now from the deadline itself onwards", () => {
    freeze();
    // Exactly on the deadline is already past — the boundary the old
    // `+ 30_000` fudge could never assert.
    expect(from(0)).toBe("now");
    expect(from(-1)).toBe("now");
    expect(from(1)).toBe("0m");
    expect(untilLabel(null)).toBe("–");
  });
});

describe("age", () => {
  it("switches unit exactly on the minute and the hour", () => {
    freeze();
    const secs = Math.floor(NOW / 1000);
    expect(age(secs)).toBe("0s ago");
    expect(age(secs - 59)).toBe("59s ago");
    expect(age(secs - 60)).toBe("1m ago");
    expect(age(secs - 3599)).toBe("59m ago");
    expect(age(secs - 3600)).toBe("1h ago");
    expect(age(secs - 7200)).toBe("2h ago");
  });

  it("never reads as a negative age when a clock runs ahead", () => {
    freeze();
    expect(age(Math.floor(NOW / 1000) + 30)).toBe("0s ago");
    expect(age(null)).toBe("–");
  });
});

describe("nowSecs", () => {
  it("is the frozen clock in whole seconds", () => {
    freeze();
    vi.setSystemTime(NOW + 900);
    expect(nowSecs()).toBe(Math.floor(NOW / 1000));
  });
});

describe("spanLabel", () => {
  it("pluralises on the exact unit boundaries", () => {
    expect(spanLabel(0)).toBe("0 seconds");
    expect(spanLabel(1)).toBe("1 second");
    expect(spanLabel(59)).toBe("59 seconds");
    expect(spanLabel(60)).toBe("1 minute");
    expect(spanLabel(3599)).toBe("59 minutes");
    expect(spanLabel(3600)).toBe("1 hour");
    expect(spanLabel(7200)).toBe("2 hours");
  });

  it("clamps a negative span rather than going negative", () => {
    expect(spanLabel(-5)).toBe("0 seconds");
  });
});

describe("ideasAgeNote", () => {
  it("stays quiet until the ideas are actually stale", () => {
    freeze();
    const secs = Math.floor(NOW / 1000);
    expect(ideasAgeNote(undefined)).toBeNull();
    expect(ideasAgeNote(0)).toBeNull();
    // One second short of the threshold is still current; the threshold is not.
    expect(ideasAgeNote(secs - 119)).toBeNull();
    expect(ideasAgeNote(secs - 120)).toBe("ideas from 2 minutes ago");
    expect(ideasAgeNote(secs - 30, 30)).toBe("ideas from 30 seconds ago");
  });
});

describe("ordinal", () => {
  it("uses -th for the teens and the suffix table otherwise", () => {
    expect(ordinal(1)).toBe("1st");
    expect(ordinal(2)).toBe("2nd");
    expect(ordinal(3)).toBe("3rd");
    expect(ordinal(4)).toBe("4th");
    expect(ordinal(9)).toBe("9th");
    expect(ordinal(11)).toBe("11th");
    expect(ordinal(12)).toBe("12th");
    expect(ordinal(13)).toBe("13th");
    expect(ordinal(21)).toBe("21st");
    expect(ordinal(112)).toBe("112th");
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
    expect(scoringFormat(null)).toBe("standard");
    expect(scoringFormat(Number.NaN)).toBe("standard");
    // An unusual value is reported as itself rather than rounded into a name.
    expect(scoringFormat(0.25)).toBe("0.25-PPR");
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

describe("injuryWord", () => {
  it("spells out each short tag for the tooltip", () => {
    expect(injuryWord("Q")).toBe("Questionable");
    expect(injuryWord("D")).toBe("Doubtful");
    expect(injuryWord("O")).toBe("Out");
  });

  it("passes anything else through rather than inventing a word", () => {
    expect(injuryWord("IR")).toBe("IR");
  });
});
