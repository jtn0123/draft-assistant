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

// Grade item G7. The banner and the strip used to own a timer each, so the
// draft subtree re-rendered twice a second out of step with itself.
describe("the banner and the strip on one clock", () => {
  it("starts a single interval for both of them", () => {
    const view = fixture();
    view.draft.clock_deadline_ms = NOW + 41_000;
    const start = vi.spyOn(window, "setInterval");

    render(
      <>
        <ClockBanner view={view} />
        <SnakeStrip view={view} />
      </>,
    );

    expect(start).toHaveBeenCalledTimes(1);
    act(() => {
      vi.advanceTimersByTime(1000);
    });
    // Both readings come from the same tick, so they cannot disagree.
    expect(screen.getAllByText("0:40")).toHaveLength(2);
    start.mockRestore();
  });

  it("does not rebuild the pick queue for a tick of the clock", () => {
    const view = fixture();
    view.draft.clock_deadline_ms = NOW + 41_000;
    const { container } = render(<SnakeStrip view={view} />);
    const chips = () => [...container.querySelectorAll(".snake-chip")];
    const before = chips();
    expect(before.length).toBeGreaterThan(0);

    // The queue's only reason to read the rosters is being rebuilt.
    const lookups = vi.spyOn(view.rosters, "map");
    act(() => {
      vi.advanceTimersByTime(1000);
    });
    expect(lookups).not.toHaveBeenCalled();
    lookups.mockRestore();
    expect(chips().map((c) => c.textContent)).toEqual(before.map((c) => c.textContent));
  });
});

// The strip does its own snake arithmetic, which knows nothing about picks
// that changed hands, third-round reversal, or keepers. The backend hands it
// the corrections; these are about the strip actually applying them.
describe("the queue under league rules the plain snake cannot see", () => {
  const chipTexts = (container: HTMLElement) =>
    [...container.querySelectorAll(".snake-chip")].map((c) => c.textContent);
  const pickLabels = (container: HTMLElement) =>
    [...container.querySelectorAll(".snake-pick")].map((p) => p.textContent);

  it("names the slot that owns a pick, not the one it started with", () => {
    const view = fixture();
    // Pick 27 is slot 2 (mine) in the snake; say it was traded to slot 5.
    view.draft.pick_slot_overrides = { "27": 5 };
    const { container } = render(<SnakeStrip view={view} />);

    const first = chipTexts(container)[0] ?? "";
    expect(first).toContain("ChrisWitz");
    expect(first).not.toContain("YOU");
  });

  it("marks a pick acquired from somebody else as mine", () => {
    const view = fixture();
    view.draft.pick_slot_overrides = { "28": 2 };
    const { container } = render(<SnakeStrip view={view} />);

    expect(chipTexts(container)[1]).toContain("YOU");
  });

  it("leaves out picks that are already in the book as keepers", () => {
    const view = fixture();
    view.draft.keeper_picks = [28, 29];
    const { container } = render(<SnakeStrip view={view} />);

    const picks = pickLabels(container);
    expect(picks[0]).toBe(pickLabel(27, view.draft.teams));
    // 28 and 29 are nobody's turn, so 30 comes next.
    expect(picks[1]).toBe(pickLabel(30, view.draft.teams));
  });

  it("still draws a plain snake when the league has no such rules", () => {
    const view = fixture();
    const { container } = render(<SnakeStrip view={view} />);

    const picks = pickLabels(container);
    expect(picks[0]).toBe(pickLabel(27, view.draft.teams));
    expect(picks[1]).toBe(pickLabel(28, view.draft.teams));
  });
});
