// What the Settings row tells the user after an import: the match counts the
// backend wrote, plus the rows it refused to rank.

import { describe, expect, it, vi } from "vitest";
import { importNote, importToast, importSecondOpinion } from "./secondOpinionImport";
import type { DraftView, SecondOpinionImport } from "./types";

function result(over: Partial<SecondOpinionImport> = {}): SecondOpinionImport {
  return {
    matched: 402,
    total: 402,
    message: "Second opinion loaded: 402 of 402 players matched",
    excluded_rows: 0,
    excluded_reason: null,
    view: {} as DraftView,
    ...over,
  };
}

describe("importToast", () => {
  it("says only the match counts when the file was clean", () => {
    expect(importToast(result())).toBe("Second opinion loaded: 402 of 402 players matched");
  });

  it("says what was left out, and why, when rows were dropped", () => {
    const text = importToast(
      result({
        excluded_rows: 60,
        excluded_reason: "50 estimated from ADP, 10 week-1 defence rankings",
      }),
    );
    expect(text).toBe(
      "Second opinion loaded: 402 of 402 players matched; 60 rows skipped " +
        "(50 estimated from ADP, 10 week-1 defence rankings)",
    );
  });

  it("counts one dropped row as a row, not rows", () => {
    expect(
      importToast(result({ excluded_rows: 1, excluded_reason: "1 estimated from ADP" })),
    ).toContain("1 row skipped (1 estimated from ADP)");
  });

  it("keeps quiet about exclusions a file from before the labels cannot report", () => {
    // An older CSV carries no provenance columns, so the backend counts none
    // and the toast reads exactly as it always did.
    expect(importToast(result({ excluded_rows: 0, excluded_reason: null }))).not.toContain(
      "skipped",
    );
  });
});

describe("importNote", () => {
  it("invites an import when nothing has been loaded", () => {
    expect(importNote(null, null)).toBe("Add a second opinion column to the board");
    expect(importNote(0, "Clay")).toBe("Add a second opinion column to the board");
  });

  it("names the source once something is loaded", () => {
    expect(importNote(1_700_000_000, "Clay")).toMatch(/^Clay loaded .* — import again to replace$/);
  });
});

describe("importSecondOpinion", () => {
  it("shows the exclusion count in the toast it raises", async () => {
    const api = await import("./api");
    const applyView = vi.fn();
    const showToast = vi.fn();
    vi.spyOn(api.api, "importSecondOpinion").mockResolvedValue(
      result({ excluded_rows: 5, excluded_reason: "3 estimated from ADP" }),
    );

    await importSecondOpinion(applyView, showToast);

    expect(applyView).toHaveBeenCalledTimes(1);
    expect(showToast).toHaveBeenCalledWith(expect.stringContaining("5 rows skipped"));
    vi.restoreAllMocks();
  });

  it("says nothing at all when the picker was closed", async () => {
    const api = await import("./api");
    const showToast = vi.fn();
    vi.spyOn(api.api, "importSecondOpinion").mockResolvedValue(null);

    await importSecondOpinion(vi.fn(), showToast);

    expect(showToast).not.toHaveBeenCalled();
    vi.restoreAllMocks();
  });
});
