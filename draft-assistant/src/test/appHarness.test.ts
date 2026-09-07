// The harness is what every App test believes about the backend, so its own
// promises are worth a test: an unstubbed call must be impossible to mistake
// for a backend that answered with nothing.

import { describe, expect, it } from "vitest";
import { harness } from "./appHarness";

describe("the mocked backend", () => {
  it("throws by name when a test calls something it never stubbed", () => {
    const { api, reset } = harness();
    reset();
    // `importSecondOpinion` is one of the calls no test needs by default.
    const call = () => {
      api.importSecondOpinion();
    };
    expect(call).toThrow(/api\.importSecondOpinion was called/);
    expect(call).toThrow(/never stubbed it/);
  });

  it("keeps a stub the test set up, and forgets it again on reset", async () => {
    const { api, reset } = harness();
    reset();
    api.importSecondOpinion.mockResolvedValue(null);
    await expect(api.importSecondOpinion()).resolves.toBeNull();
    reset();
    expect(() => {
      api.importSecondOpinion();
    }).toThrow(/never stubbed it/);
  });

  it("answers the calls every screen makes on the way up", async () => {
    const { api, reset } = harness();
    reset();
    await expect(api.chatSuggestions("draft")).resolves.toEqual([]);
    await expect(api.diagnostics()).resolves.toMatchObject({ platform: "macos aarch64" });
    await expect(api.companionStatus()).resolves.toMatchObject({ enabled: false });
  });
});
