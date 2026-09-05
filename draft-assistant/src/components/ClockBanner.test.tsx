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

  it("stops the countdown on a paused draft", () => {
    const view = fixture();
    view.draft.paused = true;
    view.draft.clock_deadline_ms = NOW + 41_000;
    const { container } = render(<SnakeStrip view={view} />);

    expect(container.querySelector(".snake-clock")).toBeNull();
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

  it("draws the last pick of the draft once, not twice", () => {
    // 14 × 15 is 210 picks, so opening at 208 leaves a queue of three — every
    // one of them already on screen. The tail chip used to repeat the last of
    // them, and the "+n" counted a negative number of hidden picks.
    const view = fixture();
    view.draft.current_pick = 208;
    const { container } = render(<SnakeStrip view={view} />);

    const picks = pickLabels(container);
    expect(picks).toEqual([208, 209, 210].map((p) => pickLabel(p, view.draft.teams)));
    expect(new Set(picks).size).toBe(picks.length);
    expect(container.querySelector(".snake-more")).toBeNull();
  });
});

describe("what the strip says is coming", () => {
  const note = (view: DraftView) =>
    render(<SnakeStrip view={view} />).container.querySelector(".snake-note")?.textContent;

  it("says you are up rather than counting zero picks ahead of you", () => {
    const view = fixture();
    view.draft.picks_until_mine = 0;
    expect(note(view)).toBe("you are on the clock");
  });

  it("marks a truncated queue as a floor rather than a total", () => {
    // The queue is built 24 deep and stops; with no turn of ours in sight,
    // "24 picks ahead" read as the whole rest of the draft.
    const view = fixture();
    view.draft.picks_until_mine = null;
    expect(note(view)).toBe("24+ picks ahead");
  });

  it("counts the picks exactly when the whole rest of the draft fits", () => {
    const view = fixture();
    view.draft.picks_until_mine = null;
    view.draft.current_pick = 208;
    expect(note(view)).toBe("3 picks ahead");
  });
});

describe("your picks and a draft with no clock", () => {
  it("says how many more of your picks it is not showing", () => {
    const view = fixture();
    view.draft.teams = 12;
    view.draft.my_next_picks = [12, 13, 36, 37, 60, 61];

    render(<ClockBanner view={view} />);
    // Four picks and a "+2": the list used to stop at four and say nothing
    // about the rest of the draft, the way the queue strip already does.
    expect(screen.getByText(/1\.12 · 2\.01 · 3\.12 · 4\.01/)).toBeInTheDocument();
    expect(screen.getByText("+2")).toBeInTheDocument();
  });

  it("says nothing extra when every one of your picks is on screen", () => {
    const view = fixture();
    view.draft.teams = 12;
    view.draft.my_next_picks = [12, 13];

    render(<ClockBanner view={view} />);
    expect(screen.queryByText(/^\+\d/)).not.toBeInTheDocument();
  });

  it("names Yahoo as the reason a live draft has no clock", () => {
    const view = fixture();
    view.league.platform = "yahoo";
    view.draft.clock_deadline_ms = null;

    render(<ClockBanner view={view} />);
    // Leaving the cell out entirely read as a timer that had not started.
    expect(screen.getByText("Clock")).toBeInTheDocument();
    expect(screen.getByText("no clock from Yahoo")).toBeInTheDocument();
  });

  it("leaves the cell out for a Sleeper draft between picks", () => {
    const view = fixture();
    view.draft.clock_deadline_ms = null;

    render(<ClockBanner view={view} />);
    expect(screen.queryByText("Clock")).not.toBeInTheDocument();
  });

  it("says nothing about a clock once a Yahoo draft is over", () => {
    const view = fixture();
    view.league.platform = "yahoo";
    view.draft.status = "complete";
    view.draft.clock_deadline_ms = null;

    render(<ClockBanner view={view} />);
    expect(screen.queryByText("no clock from Yahoo")).not.toBeInTheDocument();
  });
});

// A keeper league writes its kept picks into the draft before anybody sits
// down, so `total_picks_made` is not zero on a draft that has not started.
// The banner demanded both, decided the draft was under way, and named a
// manager as being on the clock hours before the room opened.
describe("a keeper league that has not started", () => {
  function keeperLeague(): DraftView {
    const view = fixture();
    view.draft.status = "pre_draft";
    view.draft.total_picks_made = 3;
    view.draft.keeper_picks = [1, 2, 3];
    view.draft.current_pick = 4;
    view.draft.is_my_pick = false;
    view.draft.on_clock_name = "Marla";
    view.draft.clock_deadline_ms = null;
    return view;
  }

  it("says the draft has not started rather than naming someone on the clock", () => {
    render(<ClockBanner view={keeperLeague()} />);
    expect(screen.getByText("Draft has not started")).toBeInTheDocument();
    expect(screen.queryByText(/On the clock/)).not.toBeInTheDocument();
    expect(screen.queryByText(/Marla/)).not.toBeInTheDocument();
  });

  it("tells a screen reader the same thing", () => {
    render(<ClockBanner view={keeperLeague()} />);
    expect(screen.getByRole("status")).toHaveTextContent("The draft has not started yet.");
  });

  it("leaves the Yahoo no-clock note off a draft nobody is waiting on", () => {
    const view = keeperLeague();
    view.league.platform = "yahoo";
    render(<ClockBanner view={view} />);
    expect(screen.queryByText("no clock from Yahoo")).not.toBeInTheDocument();
  });
});

// `current_pick` stays where the last pick left it, so the queue went on being
// built from it after the final selection — a strip of picks nobody would ever
// make, the first of them wearing the on-the-clock highlight.
describe("the strip once the draft is over", () => {
  it("shows no queue at all", () => {
    const view = fixture();
    view.draft.status = "complete";
    const { container } = render(<SnakeStrip view={view} />);
    expect(container.querySelector(".snake")).toBeNull();
    expect(container.querySelector(".snake-chip.is-on-clock")).toBeNull();
  });

  it("still shows the queue while the draft is only paused", () => {
    const view = fixture();
    view.draft.paused = true;
    const { container } = render(<SnakeStrip view={view} />);
    expect(container.querySelector(".snake")).not.toBeNull();
  });
});

describe("counting the picks still to come", () => {
  it("calls one remaining pick a pick, not 1 picks", () => {
    const view = fixture();
    view.draft.picks_until_mine = null;
    // The last pick of the draft is the only one left in the queue.
    view.draft.current_pick = view.draft.teams * view.draft.rounds;
    const note = render(<SnakeStrip view={view} />).container.querySelector(".snake-note");
    expect(note?.textContent).toBe("1 pick ahead");
  });
});
