import { act, renderHook } from "@testing-library/react";
import { beforeEach, describe, expect, it } from "vitest";
import { defaultViewMode, loadViewMode, useViewMode } from "./viewMode";

describe("view mode", () => {
  beforeEach(() => window.localStorage.clear());

  it("defaults to the draft until it is complete, then the season", () => {
    expect(defaultViewMode("pre_draft")).toBe("draft");
    expect(defaultViewMode("drafting")).toBe("draft");
    expect(defaultViewMode("complete")).toBe("season");
    expect(defaultViewMode(null)).toBe("draft");
  });

  it("remembers a choice per draft and falls back to the default elsewhere", () => {
    const { result, rerender } = renderHook(
      ({ id, status }: { id: string | null; status: string }) => useViewMode(id, status),
      { initialProps: { id: "d1", status: "complete" } },
    );
    expect(result.current[0]).toBe("season");

    act(() => result.current[1]("draft"));
    expect(result.current[0]).toBe("draft");
    expect(loadViewMode("d1")).toBe("draft");

    // Another draft has its own default …
    rerender({ id: "d2", status: "complete" });
    expect(result.current[0]).toBe("season");
    // … and the first one is still remembered.
    rerender({ id: "d1", status: "complete" });
    expect(result.current[0]).toBe("draft");
  });

  it("ignores junk in storage", () => {
    window.localStorage.setItem("draft-assistant.view-mode:d1", "sideways");
    expect(loadViewMode("d1")).toBeNull();
  });
});
