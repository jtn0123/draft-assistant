import { act, render, screen } from "@testing-library/react";
import { afterEach, beforeEach, describe, expect, it, vi } from "vitest";
import fixtureJson from "../../public/dev-fixture.json";
import type { DraftView } from "../types";
import { pickLabel } from "../format";
import { ClockBanner, SnakeStrip } from "./ClockBanner";

const NOW = Date.parse("2026-08-30T17:00:00Z");

function fixture(): DraftView {
  const view = structuredClone(fixtureJson) as unknown as DraftView;
  view.draft.status = "drafting";
  return view;
}

beforeEach(() => {
  vi.useFakeTimers();
  vi.setSystemTime(NOW);
});

afterEach(() => {
  vi.useRealTimers();
});

describe("pick clock", () => {
  it("counts down every second in the banner and on the on-clock chip", () => {
    const view = fixture();
    view.draft.clock_deadline_ms = NOW + 41_000;

    render(
      <>
        <ClockBanner view={view} />
        <SnakeStrip view={view} />
      </>,
    );
    expect(screen.getByText("Clock")).toBeInTheDocument();
    expect(screen.getAllByText("0:41")).toHaveLength(2);

    act(() => {
      vi.advanceTimersByTime(1000);
    });
    expect(screen.getAllByText("0:40")).toHaveLength(2);
  });

  it("clamps at zero once the deadline has passed", () => {
    const view = fixture();
    view.draft.clock_deadline_ms = NOW + 2_000;

    render(<ClockBanner view={view} />);
    act(() => {
      vi.advanceTimersByTime(5000);
    });
    expect(screen.getByText("0:00")).toBeInTheDocument();
  });

  it("shows no clock cell when nothing is on the clock", () => {
    const view = fixture();
    view.draft.clock_deadline_ms = null;

    render(<ClockBanner view={view} />);
    expect(screen.queryByText("Clock")).not.toBeInTheDocument();
  });
});

describe("what a screen reader hears", () => {
  const announcement = () => screen.getByRole("status").textContent ?? "";

  it("says whose pick it is, with the pick number and the time left", () => {
    const view = fixture();
    view.draft.is_my_pick = true;
    view.draft.clock_deadline_ms = NOW + 41_000;

    render(<ClockBanner view={view} />);
    expect(announcement()).toContain("You are on the clock");
    expect(announcement()).toContain(pickLabel(view.draft.current_pick, view.draft.teams));
    expect(announcement()).toContain("41 seconds left");
  });

  it("holds the same words while the clock ticks, so it is not read out every second", () => {
    const view = fixture();
    view.draft.is_my_pick = true;
    view.draft.clock_deadline_ms = NOW + 41_000;

    render(<ClockBanner view={view} />);
    const first = announcement();

    act(() => {
      vi.advanceTimersByTime(5000);
    });
    // The visible timer has moved on; the announced sentence has not.
    expect(screen.getByText("0:36")).toBeInTheDocument();
    expect(announcement()).toBe(first);
  });

  it("says something new when the pick changes hands", () => {
    const view = fixture();
    view.draft.is_my_pick = false;
    view.draft.on_clock_name = "Team Rocket";
    view.draft.clock_deadline_ms = NOW + 30_000;

    const { rerender } = render(<ClockBanner view={view} />);
    expect(announcement()).toContain("Team Rocket is on the clock");

    const mine = fixture();
    mine.draft.is_my_pick = true;
    mine.draft.current_pick = view.draft.current_pick + 1;
    mine.draft.clock_deadline_ms = NOW + 30_000;
    rerender(<ClockBanner view={mine} />);
    expect(announcement()).toContain("You are on the clock");
  });

  it("says the draft is finished rather than leaving the colour to say it", () => {
    const view = fixture();
    view.draft.status = "complete";

    render(<ClockBanner view={view} />);
    expect(announcement()).toContain("The draft is finished.");
  });
});
