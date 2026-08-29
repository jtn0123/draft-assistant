import { describe, expect, it, vi } from "vitest";
import { transientNetworkError, withRetry } from "./launch";

describe("launch retry", () => {
  it("tells a stalled connect from a bad league id", () => {
    expect(
      transientNetworkError(
        "not a league (request failed: https://api.sleeper.app/v1/league/1: error sending request for url: client error (Connect): operation timed out)",
      ),
    ).toBe(true);
    expect(transientNetworkError("not a league (HTTP 404 Not Found)")).toBe(false);
    expect(transientNetworkError("Incompatible draft data: schema 1.3")).toBe(false);
  });

  it("retries a transient failure and returns the eventual result", async () => {
    const run = vi
      .fn<() => Promise<string>>()
      .mockRejectedValueOnce(new Error("operation timed out"))
      .mockRejectedValueOnce(new Error("error sending request"))
      .mockResolvedValue("loaded");
    await expect(withRetry(run, [1, 1], transientNetworkError)).resolves.toBe("loaded");
    expect(run).toHaveBeenCalledTimes(3);
  });

  it("gives up after the delays run out, and at once on anything else", async () => {
    const stalled = vi.fn<() => Promise<string>>().mockRejectedValue(new Error("operation timed out"));
    await expect(withRetry(stalled, [1], transientNetworkError)).rejects.toThrow("timed out");
    expect(stalled).toHaveBeenCalledTimes(2);

    const bad = vi.fn<() => Promise<string>>().mockRejectedValue(new Error("HTTP 404"));
    await expect(withRetry(bad, [1, 1], transientNetworkError)).rejects.toThrow("404");
    expect(bad).toHaveBeenCalledTimes(1);
  });
});
