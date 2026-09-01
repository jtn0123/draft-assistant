import { beforeEach, describe, expect, it } from "vitest";
import { chatBudget, formatUsd, overBudget, resetChatBudget, setChatBudget } from "./chatCost";

beforeEach(() => {
  localStorage.clear();
  resetChatBudget();
});

// What a turn costs is priced in `chat.rs`, which is also what enforces the
// cap; there is no second price table here to test.
describe("what an answer cost", () => {
  it("shows fractions of a cent rather than $0.00", () => {
    expect(formatUsd(0.42)).toBe("$0.42");
    expect(formatUsd(0.004)).toBe("$0.004");
    expect(formatUsd(0)).toBe("$0.00");
  });
});

describe("the spend cap", () => {
  it("starts at five dollars and remembers a change", () => {
    expect(chatBudget()).toBe(5);
    setChatBudget(12);
    expect(chatBudget()).toBe(12);
    expect(localStorage.getItem("da.chatBudget")).toBe("12");
  });

  it("refuses a nonsense cap instead of storing it", () => {
    setChatBudget(Number.NaN);
    expect(chatBudget()).toBe(0);
    localStorage.setItem("da.chatBudget", "lots");
    resetChatBudget();
    expect(chatBudget()).toBe(5);
  });

  it("stops asking at the cap, and never when there is none", () => {
    expect(overBudget(4.99, 5)).toBe(false);
    expect(overBudget(5, 5)).toBe(true);
    expect(overBudget(500, 0)).toBe(false);
  });
});
