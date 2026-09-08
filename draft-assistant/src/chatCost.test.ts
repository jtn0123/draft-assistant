import { describe, expect, it } from "vitest";
import { formatUsd } from "./chatCost";

// Prices come from the backend; this only verifies their display.
describe("estimated usage cost", () => {
  it("shows fractions of a cent rather than rounding every small estimate to zero", () => {
    expect(formatUsd(0.42)).toBe("$0.42");
    expect(formatUsd(0.004)).toBe("$0.004");
    expect(formatUsd(0)).toBe("$0.00");
    expect(formatUsd(12.34)).toBe("$12.34");
  });
});
