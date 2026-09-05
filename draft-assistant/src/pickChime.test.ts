// When the app is allowed to make a sound.

import { renderHook } from "@testing-library/react";
import { beforeEach, describe, expect, it, vi } from "vitest";
import { draftFixture } from "./test/appHarness";
import type { DraftView } from "./types";

const mocks = vi.hoisted(() => ({ playChime: vi.fn() }));
vi.mock("./chime", () => ({ playChime: mocks.playChime }));

import { usePickChime } from "./pickChime";

/** The fixture with the clock in a given state. */
function board(mine: boolean, paused: boolean): DraftView {
  const view = draftFixture(8);
  return { ...view, draft: { ...view.draft, is_my_pick: mine, paused } };
}

beforeEach(() => {
  vi.clearAllMocks();
});

describe("the on-the-clock chime", () => {
  it("sounds once when the clock reaches you", () => {
    const { rerender } = renderHook(({ view }) => usePickChime(view, true), {
      initialProps: { view: board(false, false) },
    });
    rerender({ view: board(true, false) });
    expect(mocks.playChime).toHaveBeenCalledTimes(1);

    // A new view of the same situation is not a new turn.
    rerender({ view: board(true, false) });
    expect(mocks.playChime).toHaveBeenCalledTimes(1);
  });

  it("stays quiet while the draft is paused", () => {
    // Sleeper keeps reporting `is_my_pick` through a pause, so a room that
    // stopped for twenty minutes had the app chiming at whoever was next as
    // if it were their turn.
    const { rerender } = renderHook(({ view }) => usePickChime(view, true), {
      initialProps: { view: board(false, true) },
    });
    rerender({ view: board(true, true) });
    expect(mocks.playChime).not.toHaveBeenCalled();

    // And it sounds when the commissioner starts the clock again.
    rerender({ view: board(true, false) });
    expect(mocks.playChime).toHaveBeenCalledTimes(1);
  });

  it("says nothing when the user turned the chime off", () => {
    const { rerender } = renderHook(({ view }) => usePickChime(view, false), {
      initialProps: { view: board(false, false) },
    });
    rerender({ view: board(true, false) });
    expect(mocks.playChime).not.toHaveBeenCalled();
  });
});
