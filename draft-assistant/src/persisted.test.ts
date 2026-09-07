// A remembered preference, and what happens to one that no longer parses.

import { beforeEach, describe, expect, it, vi } from "vitest";

const reportError = vi.hoisted(() => vi.fn());
vi.mock("./errorReport", () => ({ reportError }));

import { persisted } from "./persisted";

type Density = "cozy" | "compact";
const parse = (raw: string): Density | null => (raw === "cozy" || raw === "compact" ? raw : null);

beforeEach(() => {
  localStorage.clear();
  reportError.mockReset();
});

describe("a remembered preference", () => {
  it("reads what was stored, and the fallback when nothing was", () => {
    expect(persisted("da.density", parse, "cozy").get()).toBe("cozy");
    localStorage.setItem("da.density", "compact");
    expect(persisted("da.density", parse, "cozy").get()).toBe("compact");
    expect(reportError).not.toHaveBeenCalled();
  });

  it("remembers a choice and tells whoever is listening", () => {
    const store = persisted<Density>("da.density", parse, "cozy");
    const listener = vi.fn();
    store.subscribe(listener);
    store.set("compact");
    expect(listener).toHaveBeenCalledTimes(1);
    expect(localStorage.getItem("da.density")).toBe("compact");
    expect(store.get()).toBe("compact");
  });
});

describe("a stored value that is not one this app writes", () => {
  it("is reset to the fallback, and the reset is reported", () => {
    // The failure this prevents: the app went back to its default for no
    // reason anyone could see, with nothing in the log to say a stored value
    // had been rejected. The value itself stays out of the line.
    localStorage.setItem("da.density", "spacious!!");
    expect(persisted("da.density", parse, "cozy").get()).toBe("cozy");
    expect(reportError).toHaveBeenCalledWith(
      'Stored da.density was not a value this app writes (10 chars); reset to "cozy"',
      "persisted",
    );
    const [message] = reportError.mock.calls[0] as [string];
    expect(message).not.toContain("spacious");
  });

  it("holds the choice made this session even when storage refuses the write", () => {
    vi.stubGlobal("localStorage", {
      getItem: () => "spacious!!",
      setItem: () => {
        throw new Error("QuotaExceededError");
      },
    });
    const store = persisted<Density>("da.density", parse, "cozy");
    store.set("compact");
    expect(store.get()).toBe("compact");
    // Dropping the session's choice goes back to storage, which still holds
    // the rejected value: the fallback, reported once and not again.
    store.reset();
    expect(store.get()).toBe("cozy");
    vi.unstubAllGlobals();
  });
});
