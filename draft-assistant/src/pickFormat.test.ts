import { describe, it, expect, beforeEach } from "vitest";
import { formatPick, loadPickStyle, roundDot, savePickStyle } from "./pickFormat";

describe("roundDot", () => {
  // The real league: 14 teams, drafting from slot 2. Both of that slot's picks
  // in a round pair land on the same round number, one at each end of it.
  it("counts the pick within its round, not the drafting slot", () => {
    expect(roundDot(2, 14)).toBe("1.2");
    expect(roundDot(27, 14)).toBe("2.13");
    expect(roundDot(30, 14)).toBe("3.2");
    expect(roundDot(55, 14)).toBe("4.13");
    expect(roundDot(58, 14)).toBe("5.2");
  });

  it("puts the last pick of a round at the end of that round", () => {
    expect(roundDot(14, 14)).toBe("1.14");
    expect(roundDot(15, 14)).toBe("2.1");
  });

  it("falls back to the raw number when the draft shape is unusable", () => {
    expect(roundDot(55, 0)).toBe("55");
  });
});

describe("formatPick", () => {
  it("leaves the overall number alone in overall style", () => {
    expect(formatPick(55, 14, "overall")).toBe("55");
    expect(formatPick(55, 14, "round")).toBe("4.13");
  });
});

describe("the stored preference", () => {
  beforeEach(() => window.localStorage.clear());

  // Overall matches what Sleeper itself shows, so a first run never disagrees
  // with the other window.
  it("defaults to overall", () => {
    expect(loadPickStyle()).toBe("overall");
  });

  it("survives a round trip", () => {
    savePickStyle("round");
    expect(loadPickStyle()).toBe("round");
    savePickStyle("overall");
    expect(loadPickStyle()).toBe("overall");
  });
});
