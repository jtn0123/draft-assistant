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
  // The fixture is a dump taken on a real evening; the countdown is corrected
  // for the gap between the host's clock and this one, so a view stamped two
  // days ago would read as a two-day skew. Stamped now, as a live view is.
  view.generated_at = Math.floor(NOW / 1000);
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

  // The wrong number this ends: the deadline comes from the platform's own
  // stamps, read by the host, and used to be counted down against this
  // device's `Date.now()` with nothing in between. A laptop two minutes fast
  // showed 0:00 while the drafter still had ninety seconds.
  it("corrects the countdown for a device clock that is fast, and says so", () => {
    const view = fixture();
    view.draft.clock_deadline_ms = NOW + 41_000;
    // This device is two minutes ahead of the host, so `Date.now()` here is
    // NOW while the host stamped the view at NOW minus two minutes.
    view.generated_at = Math.floor((NOW - 120_000) / 1000);

    render(<ClockBanner view={view} />);
    expect(screen.getByText("2:41")).toBeInTheDocument();
    expect(screen.getByText(/2 minutes ahead of the draft host/)).toBeInTheDocument();
  });

  it("corrects it the other way for a device clock that is slow", () => {
    const view = fixture();
    view.draft.clock_deadline_ms = NOW + 41_000;
    view.generated_at = Math.floor((NOW + 120_000) / 1000);

    render(<ClockBanner view={view} />);
    expect(screen.getByText("0:00")).toBeInTheDocument();
    expect(screen.getByText(/2 minutes behind the draft host/)).toBeInTheDocument();
  });

  it("leaves an ordinary few seconds of poll latency alone", () => {
    const view = fixture();
    view.draft.clock_deadline_ms = NOW + 41_000;
    view.generated_at = Math.floor((NOW - 3_000) / 1000);

    render(<ClockBanner view={view} />);
    expect(screen.getByText("0:41")).toBeInTheDocument();
    expect(screen.queryByText(/draft host/)).not.toBeInTheDocument();
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

  it("treats a mock draft that still says pre_draft as live once real picks are in", () => {
    // Sleeper mock drafts keep `pre_draft` on the wire while picks are made;
    // the dev fixture is one, at pick 27 with no keepers.
    const view = fixture();
    view.draft.status = "pre_draft";
    view.draft.keeper_picks = [];
    view.draft.total_picks_made = 26;
    view.draft.is_my_pick = false;
    view.draft.on_clock_name = "Team Rocket";

    render(<ClockBanner view={view} />);
    expect(screen.getByText(/On the clock/)).toBeInTheDocument();
    expect(screen.queryByText(/has not started/)).not.toBeInTheDocument();
  });

  it("does not glow green for my pick while the draft is paused", () => {
    const view = fixture();
    view.draft.status = "paused";
    view.draft.paused = true;
    view.draft.is_my_pick = true;

    const { container } = render(<ClockBanner view={view} />);
    expect(container.querySelector(".clock")).not.toHaveClass("is-mine");
    expect(screen.getByText("Draft paused")).toBeInTheDocument();
  });

  it("says the draft is paused instead of naming a manager who cannot act", () => {
    const view = fixture();
    view.draft.status = "paused";
    view.draft.paused = true;
    view.draft.is_my_pick = false;
    view.draft.on_clock_name = "Team Rocket";
    // Sleeper withholds the deadline on a paused draft, but an older host may
    // not: either way nothing counts down while the draft is stopped.
    view.draft.clock_deadline_ms = NOW + 41_000;

    render(<ClockBanner view={view} />);
    expect(screen.getByText("Draft paused")).toBeInTheDocument();
    expect(screen.queryByText(/On the clock/)).not.toBeInTheDocument();
    expect(screen.queryByText("Clock")).not.toBeInTheDocument();
    expect(screen.queryByText("0:41")).not.toBeInTheDocument();
    expect(screen.getByRole("status").textContent).toContain("The draft is paused.");
  });

  it("shows no clock cell when nothing is on the clock", () => {
    const view = fixture();
    view.draft.clock_deadline_ms = null;

    render(<ClockBanner view={view} />);
    expect(screen.queryByText("Clock")).not.toBeInTheDocument();
  });
});

// An auction is not a draft this app can read: there is no pick order, so
// naming a manager as on the clock and counting picks until your turn are
// both snake arithmetic on a draft that has neither.
describe("an auction draft", () => {
  it("says so instead of naming a manager on the clock", () => {
    const view = fixture();
    view.draft.is_auction = true;
    view.draft.on_clock_name = null;
    view.draft.picks_until_mine = null;
    view.draft.my_next_picks = [];
    view.draft.clock_deadline_ms = null;

    render(
      <>
        <ClockBanner view={view} />
        <SnakeStrip view={view} />
      </>,
    );
    expect(screen.getByText("Auction draft")).toBeInTheDocument();
    expect(screen.queryByText(/On the clock/)).not.toBeInTheDocument();
    expect(screen.queryByText(/until you/)).not.toBeInTheDocument();
    expect(screen.queryByText("Your picks")).not.toBeInTheDocument();
    expect(screen.queryByText("Up next")).not.toBeInTheDocument();
    expect(screen.queryByText("Clock")).not.toBeInTheDocument();
  });

  it("tells a screen reader the same thing", () => {
    const view = fixture();
    view.draft.is_auction = true;
    const { container } = render(<ClockBanner view={view} />);
    expect(container.querySelector(".sr-only")?.textContent).toContain(
      "auction draft, which this app does not support",
    );
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
