import { act, fireEvent, render, screen } from "@testing-library/react";
import { afterEach, beforeEach, describe, expect, it, vi } from "vitest";
import type { SourceHealth } from "../season-types";
import { FROZEN, NOW, fresh, matchup, myChip, view } from "./season-screen-fixture";
import { ODDS_NOTE } from "../odds";
import { SeasonScreen } from "./SeasonScreen";

beforeEach(() => {
  vi.useFakeTimers();
  vi.setSystemTime(FROZEN);
});

afterEach(() => {
  vi.useRealTimers();
});

describe("the odds", () => {
  // A bare "62%" reads as a promise. The note is what makes it a model, so it
  // has to be next to the number rather than buried in a tooltip or a doc.
  it("says what the win odds were calibrated on, in one muted line", () => {
    render(<SeasonScreen view={view()} />);
    const note = screen.getByText(new RegExp(ODDS_NOTE));
    expect(note).toHaveClass("muted");
    expect(note.closest(".season-stat")).toHaveTextContent("Win odds");
  });

  // fireEvent rather than userEvent: this file runs on a frozen clock, which
  // userEvent's own delays would sit and wait on forever.
  it("prices the header off the lineup the screen is showing", () => {
    render(<SeasonScreen view={view({ matchup: matchup() })} />);

    // Best: the projection and the percentage are both the best lineup's.
    expect(screen.getByText("vs punt_god · 122.4 – 108.9")).toBeInTheDocument();
    expect(screen.getByText("62%")).toBeInTheDocument();
    expect(screen.getByText(/^best lineup · /)).toBeInTheDocument();

    // Switching the panel moves the header with it. The screen used to say
    // "62% to win" while the same screen said points were on the bench.
    fireEvent.click(screen.getByRole("button", { name: "Set" }));
    expect(screen.getByText("vs punt_god · 118.1 – 108.9")).toBeInTheDocument();
    expect(screen.getByText("55%")).toBeInTheDocument();
    expect(screen.queryByText("62%")).not.toBeInTheDocument();
    expect(screen.getByText(/^lineup as set · /)).toBeInTheDocument();
  });
});

describe("the live badge", () => {
  it("calls itself live and dates every source when all three are current", () => {
    render(<SeasonScreen view={view()} />);
    const badge = screen.getByText(/^Live · /);
    expect(badge).toHaveClass("pill-live");
    const title = badge.getAttribute("title") ?? "";
    expect(title).toContain("Matchups: 5 seconds ago");
    expect(title).toContain("Scores: 5 seconds ago");
    expect(title).toContain("Rosters: 5 seconds ago");
  });

  it("names the source that is behind and says why, without going fully red", () => {
    const sources = fresh();
    sources.rosters = { last_success_secs: NOW() - 720, error: "timeout" };
    render(
      <SeasonScreen view={view({ data_health: { fetched_at: NOW(), warnings: [], sources } })} />,
    );

    const badge = screen.getByText("Live · rosters behind");
    // Warning-ish, not the all-clear: two feeds are still arriving.
    expect(badge).toHaveClass("pill-stale");
    const title = badge.getAttribute("title") ?? "";
    expect(title).toContain("Rosters: failing for 12 minutes (timeout)");
    expect(title).toContain("Scores: 5 seconds ago");
    expect(title).toContain("Matchups: 5 seconds ago");
  });

  it("writes the source breakdown under the badge once something is behind", () => {
    const sources = fresh();
    sources.rosters = { last_success_secs: NOW() - 720, error: "timeout" };
    render(
      <SeasonScreen view={view({ data_health: { fetched_at: NOW(), warnings: [], sources } })} />,
    );

    // No hovering required: which feed, and how far behind, is on the page.
    expect(screen.getByText("Rosters: failing for 12 minutes (timeout)")).toBeInTheDocument();
    expect(screen.getByText("Scores: 5 seconds ago")).toBeInTheDocument();
    expect(screen.getByText("Matchups: 5 seconds ago")).toBeInTheDocument();
  });

  // The exact threshold, which a live clock can only ever straddle by luck.
  it("still vouches for a source at ninety seconds and gives up at ninety-one", () => {
    const at = (behindSecs: number) => {
      const sources = fresh();
      sources.rosters = { last_success_secs: NOW() - behindSecs, error: null };
      const { unmount } = render(
        <SeasonScreen view={view({ data_health: { fetched_at: NOW(), warnings: [], sources } })} />,
      );
      const label = screen.getByText(/^Live · |^Not updating$/).textContent;
      unmount();
      return label;
    };
    expect(at(90)).toBe("Live · 0s ago");
    expect(at(91)).toBe("Live · rosters behind");
  });

  it("keeps the breakdown out of the way while every source is current", () => {
    render(<SeasonScreen view={view()} />);
    expect(screen.queryByText("Scores: 5 seconds ago")).not.toBeInTheDocument();
  });

  it("stops claiming to be live when nothing at all is arriving", () => {
    const sources: SourceHealth = {
      matchups: { last_success_secs: NOW() - 900, error: "503" },
      scores: { last_success_secs: NOW() - 900, error: "503" },
      rosters: { last_success_secs: NOW() - 900, error: "503" },
    };
    render(
      <SeasonScreen view={view({ data_health: { fetched_at: NOW(), warnings: [], sources } })} />,
    );
    expect(screen.getByText("Not updating")).toHaveClass("pill-stale");
  });

  // The badge is the one thing on the screen whose job is to notice an
  // absence, so it has to keep moving when nothing else does.
  it("stops calling itself live when time passes and no new data arrives", async () => {
    render(<SeasonScreen view={view()} />);
    expect(screen.getByText(/^Live · /)).toHaveClass("pill-live");

    // Well past the point where all three sources are behind, with no re-render
    // from new data — only the badge's own heartbeat.
    await act(async () => {
      await vi.advanceTimersByTimeAsync(120_000);
    });
    expect(screen.getByText("Not updating")).toHaveClass("pill-stale");
  });

  it("goes stale on the overall stamp too, when there is no per-source detail", async () => {
    render(<SeasonScreen view={view({ data_health: { fetched_at: NOW(), warnings: [] } })} />);
    expect(screen.getByText(/^Live · /)).toHaveClass("pill-live");

    await act(async () => {
      await vi.advanceTimersByTimeAsync(120_000);
    });
    expect(screen.getByText("Not updating")).toHaveClass("pill-stale");
  });

  it("falls back to the overall stamp for a view with no per-source detail", () => {
    render(<SeasonScreen view={view({ data_health: { fetched_at: NOW() - 4, warnings: [] } })} />);
    const badge = screen.getByText("Live · 4s ago");
    expect(badge).toHaveClass("pill-live");
    expect(badge).not.toHaveAttribute("title");
  });
});

describe("a failing live poll", () => {
  it("says the feed is failing, and why, even while the stamps still look fresh", () => {
    render(
      <SeasonScreen
        view={view()}
        pollHealth={{
          last_success_at: NOW() - 300,
          consecutive_failures: 3,
          last_error: "scores: request failed",
        }}
      />,
    );
    // The badge stops vouching for the data even though every source stamp is
    // five seconds old — the poller knows better than the timestamps do.
    expect(screen.getByText("Not updating")).toHaveClass("pill-stale");
    expect(screen.getByText(/The last 3 tries to get new scores failed/)).toBeInTheDocument();
    expect(screen.getByText(/5 minutes ago/)).toBeInTheDocument();
    expect(screen.getByText(/scores: request failed/)).toBeInTheDocument();
  });

  it("counts a single failure in the singular, and says when nothing has arrived yet", () => {
    render(
      <SeasonScreen
        view={view()}
        pollHealth={{ last_success_at: null, consecutive_failures: 1, last_error: null }}
      />,
    );
    expect(
      screen.getByText("The last try to get new scores failed — no scores have come through yet"),
    ).toBeInTheDocument();
  });

  it("says nothing at all while the poll is getting through", () => {
    render(
      <SeasonScreen
        view={view()}
        pollHealth={{ last_success_at: NOW(), consecutive_failures: 0, last_error: null }}
      />,
    );
    expect(screen.getByText(/^Live · /)).toHaveClass("pill-live");
    expect(screen.queryByText(/to get new scores failed/)).not.toBeInTheDocument();
  });

  it("keeps the stale badge but adds the reason when the sources are behind too", () => {
    const sources = fresh();
    sources.scores = { last_success_secs: NOW() - 600, error: "timeout" };
    sources.matchups = { last_success_secs: NOW() - 600, error: "timeout" };
    sources.rosters = { last_success_secs: NOW() - 600, error: "timeout" };
    render(
      <SeasonScreen
        view={view({ data_health: { fetched_at: NOW() - 600, warnings: [], sources } })}
        pollHealth={{
          last_success_at: NOW() - 600,
          consecutive_failures: 2,
          last_error: "timeout",
        }}
      />,
    );
    // One status, not two: a single stale pill plus one sentence of reason.
    expect(screen.getAllByText("Not updating")).toHaveLength(1);
    expect(screen.getByText(/The last 2 tries to get new scores failed/)).toBeInTheDocument();
  });
});

describe("the age of cached ideas", () => {
  it("says how old the waiver ideas are once they stop being current", () => {
    render(<SeasonScreen view={view({ analysis_as_of_secs: NOW() - 420 })} />);
    expect(screen.getByText(/ideas from 7 minutes ago/)).toBeInTheDocument();
  });

  it("says nothing while the ideas are still fresh", () => {
    render(<SeasonScreen view={view({ analysis_as_of_secs: NOW() - 60 })} />);
    expect(screen.queryByText(/ideas from/)).not.toBeInTheDocument();
    // And nothing at all when the backend did not stamp the view.
    render(<SeasonScreen view={view()} />);
    expect(screen.queryByText(/ideas from/)).not.toBeInTheDocument();
  });
});

describe("SeasonScreen header", () => {
  /** The fixture's live section, with one game in whatever state is asked for
   *  and some points already banked. */
  function liveSection(state: "pre" | "live" | "final") {
    return {
      games: [
        {
          game_id: "g1",
          away: "SF",
          home: "LAR",
          away_score: null,
          home_score: null,
          state,
          status: state === "live" ? "Q3 07:12" : state === "final" ? "Final" : "",
          kickoff_ms: FROZEN,
          flag: null,
          channel: null,
          chips: [myChip(state === "pre" ? "pre" : state === "live" ? "playing" : "done")],
        },
      ],
      windows: [],
      totals: { my_playing: 1, my_pre: 0, my_done: 0, my_live_points: 71.5, opp_live_points: 64.2 },
      next_kickoff_ms: null,
      bye_teams: [],
    };
  }

  it("leads with the projection while every game is still to come", () => {
    render(<SeasonScreen view={view({ live: liveSection("pre") })} pollHealth={null} />);
    expect(screen.getByText("vs punt_god · 122.4 – 108.9")).toBeInTheDocument();
    expect(screen.getByText("This week")).toBeInTheDocument();
  });

  /** The bug: all Sunday the header quoted projected against projected, with
   *  the score that had actually happened buried in another tab. */
  it("leads with the live score once anything has kicked off", () => {
    render(<SeasonScreen view={view({ live: liveSection("live") })} pollHealth={null} />);
    expect(screen.getByText("vs punt_god · 71.5 – 64.2")).toBeInTheDocument();
    expect(screen.getByText("This week · live")).toBeInTheDocument();
    // The projection beside it is the lineup that is actually playing: with
    // every game under way and no call left, the best lineup is not a thing
    // anybody can still set.
    expect(screen.getByText("live · projected 118.1 – 108.9")).toBeInTheDocument();
  });

  it("keeps the live lead once the games are final", () => {
    render(<SeasonScreen view={view({ live: liveSection("final") })} pollHealth={null} />);
    expect(screen.getByText("vs punt_god · 71.5 – 64.2")).toBeInTheDocument();
  });

  /** Past the last regular week the simulation short-circuits to a flat 100%
   *  or 0%, which read as a wildly confident forecast. */
  it("shows the bracket state instead of a playoff percentage once it is set", () => {
    render(<SeasonScreen view={view()} pollHealth={null} />);
    expect(screen.getByText("88%")).toBeInTheDocument();

    const done = view();
    done.header.playoff_status = "In the playoffs — seed 3";
    render(<SeasonScreen view={done} pollHealth={null} />);
    expect(screen.getByText("In the playoffs — seed 3")).toBeInTheDocument();
  });
});
