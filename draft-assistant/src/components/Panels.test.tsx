import { act, render, screen } from "@testing-library/react";
import { afterEach, beforeEach, describe, expect, it, vi } from "vitest";
import fixtureJson from "../../public/dev-fixture.json";
import type { DraftView } from "../types";
import { ClockBanner } from "./Panels";

function view(): DraftView {
  return structuredClone(fixtureJson) as unknown as DraftView;
}

describe("ClockBanner pick clock", () => {
  beforeEach(() => {
    vi.useFakeTimers();
    vi.setSystemTime(new Date("2026-08-28T17:10:00-07:00"));
  });
  afterEach(() => vi.useRealTimers());

  it("counts down to the pick deadline and ticks every second", () => {
    const v = view();
    v.draft.status = "drafting";
    v.draft.pick_deadline = Date.now() + 47_000;
    render(<ClockBanner view={v} />);
    const clock = screen.getByLabelText("Pick clock");
    expect(clock).toHaveTextContent("0:47");
    act(() => vi.advanceTimersByTime(1_000));
    expect(clock).toHaveTextContent("0:46");
    act(() => vi.advanceTimersByTime(46_000));
    expect(clock).toHaveTextContent("0:00");
  });

  it("shows the scheduled start before the draft begins", () => {
    const v = view();
    v.draft.status = "pre_draft";
    v.draft.total_picks_made = 0;
    v.draft.start_time = new Date("2026-08-28T17:00:00-07:00").getTime();
    render(<ClockBanner view={v} />);
    expect(screen.getByText(/Draft has not started/)).toHaveTextContent(/starts .*5:00/);
  });

  it("shows no clock when the draft has none", () => {
    const v = view();
    v.draft.status = "drafting";
    v.draft.pick_deadline = null;
    render(<ClockBanner view={v} />);
    expect(screen.queryByLabelText("Pick clock")).not.toBeInTheDocument();
  });
});
