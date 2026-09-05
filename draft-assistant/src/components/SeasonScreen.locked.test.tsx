import { act, render, screen } from "@testing-library/react";
import { afterEach, beforeEach, describe, expect, it, vi } from "vitest";
import { SeasonScreen } from "./SeasonScreen";
import { FROZEN, liveGame, lockedView, matchup, view } from "./season-screen-fixture";

beforeEach(() => {
  vi.useFakeTimers();
  vi.setSystemTime(FROZEN);
});

afterEach(() => {
  vi.useRealTimers();
});

describe("once the lineup is locked", () => {
  // The bug: the header defaulted to the best lineup's odds forever, so all
  // Sunday afternoon it quoted a percentage for a lineup nobody could set any
  // more — 62% off a bench the user could no longer touch.
  it("quotes the lineup that is actually playing, and says so", () => {
    render(<SeasonScreen view={lockedView()} />);

    expect(screen.getByText("55%")).toBeInTheDocument();
    expect(screen.queryByText("62%")).not.toBeInTheDocument();
    expect(screen.getByText(/^lineup as set · locked · /)).toBeInTheDocument();
  });

  it("takes away the best/set toggle, because it is not a choice any more", () => {
    render(<SeasonScreen view={lockedView()} />);

    expect(screen.queryByRole("button", { name: "Best" })).not.toBeInTheDocument();
    expect(screen.queryByRole("button", { name: "Set" })).not.toBeInTheDocument();
    expect(screen.getByText("lineup locked")).toBeInTheDocument();
  });

  /** The bug: "locked" was `a game has started && no calls to make`, so a
   *  Thursday night kickoff with an already-optimal lineup locked the whole
   *  screen — three days before any of Sunday's starters had taken the
   *  field, and while every one of those swaps was still available. */
  it("is not locked at the Thursday kickoff while Sunday starters are still on the bench", () => {
    const base = view({ matchup: matchup() });
    const thursday = {
      ...base,
      calls: [],
      live: {
        ...base.live,
        games: [liveGame("playing", "phi-tb"), liveGame("pre", "sf-lar")],
      },
    };
    render(<SeasonScreen view={thursday} />);

    expect(screen.getByText(/^best lineup · /)).toBeInTheDocument();
    expect(screen.queryByText(/locked/)).not.toBeInTheDocument();
    expect(screen.getByRole("button", { name: "Set" })).toBeInTheDocument();
  });

  /** And by the late Sunday window, with every one of my starters playing or
   *  finished, it really is locked however few calls there were. */
  it("is locked once every starter of mine has kicked off", () => {
    const base = view({ matchup: matchup() });
    const sunday = {
      ...base,
      calls: [],
      live: {
        ...base.live,
        games: [liveGame("done", "phi-tb"), liveGame("playing", "sf-lar")],
      },
    };
    render(<SeasonScreen view={sunday} />);

    expect(screen.getByText(/^lineup as set · locked · /)).toBeInTheDocument();
    expect(screen.queryByRole("button", { name: "Set" })).not.toBeInTheDocument();
  });

  // And before kickoff nothing changes: the choice is still the user's.
  it("leaves the toggle alone while a call can still be made", () => {
    render(<SeasonScreen view={view({ matchup: matchup() })} />);
    expect(screen.getByRole("button", { name: "Set" })).toBeInTheDocument();
    expect(screen.getByText("62%")).toBeInTheDocument();
  });
});

describe("the lock countdown", () => {
  // It used to read the clock inside a render that only happened when new
  // scores arrived, so on a quiet evening "Locks in 2h 0m" sat there saying
  // 2h 0m for hours.
  it("counts down on its own without waiting for new data", () => {
    render(
      <SeasonScreen
        view={view({ header: { ...view().header, locks_in_ms: FROZEN + 7_200_000 } })}
      />,
    );
    expect(screen.getByText("2h 0m")).toBeInTheDocument();

    act(() => {
      vi.advanceTimersByTime(11 * 60_000);
    });
    expect(screen.getByText("1h 49m")).toBeInTheDocument();
  });
});
