import { describe, expect, it } from "vitest";
import type { DraftView } from "./types";
import { validateDraftView } from "./api";

describe("DraftView schema guard", () => {
  it("accepts the current schema", () => {
    const view = { schema_version: "1.1" } as DraftView;
    expect(validateDraftView(view)).toBe(view);
  });

  it("rejects stale data with an actionable message", () => {
    const view = { schema_version: "1.0" } as DraftView;
    expect(() => validateDraftView(view)).toThrow(
      "expected schema 1.1, received 1.0",
    );
  });
});
