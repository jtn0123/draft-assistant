import { act, render, screen, within } from "@testing-library/react";
import { afterEach, beforeEach, describe, expect, it, vi } from "vitest";
import fixtureJson from "../../public/dev-fixture.json";
import type { DraftView } from "../types";
import { ClockBanner, SidePanel } from "./Panels";

function view(): DraftView {
  return structuredClone(fixtureJson) as unknown as DraftView;
}

/** The banner renders a local time; CI runs in UTC and this laptop does not. */
function localTime(ms: number): string {
  return new Date(ms).toLocaleTimeString([], { hour: "numeric", minute: "2-digit" });
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

  // Dogfood pass 2, ISSUE-P2-001: keepers count as picks made, so a league
  // that has not started looked like one in progress — ten hours early.
  it("still says the draft has not started when keepers are already in the book", () => {
    const v = view();
    v.draft.status = "pre_draft";
    v.draft.current_pick = 1;
    v.draft.total_picks_made = 25;
    v.draft.is_my_pick = false;
    v.draft.on_clock_name = "197lbsleanmeandadbod";
    v.draft.start_time = Date.now() + 20 * 60_000;
    render(<ClockBanner view={v} />);
    const status = screen.getByRole("status");
    expect(status).toHaveTextContent(/Draft has not started/);
    expect(status).toHaveTextContent(`starts ${localTime(v.draft.start_time)}`);
    expect(status).not.toHaveTextContent(/On the clock/);
    expect(status).not.toHaveTextContent(/pick until you/);
  });

  it("shows the clock once the draft is genuinely under way", () => {
    const v = view();
    v.draft.status = "pre_draft"; // Sleeper lags the status behind the first pick
    v.draft.current_pick = 4;
    v.draft.total_picks_made = 28;
    v.draft.is_my_pick = false;
    v.draft.on_clock_name = "adaigle";
    render(<ClockBanner view={v} />);
    expect(screen.getByRole("status")).toHaveTextContent("On the clock: adaigle");
  });

  it("shows the scheduled start before the draft begins", () => {
    const v = view();
    v.draft.status = "pre_draft";
    v.draft.total_picks_made = 0;
    v.draft.current_pick = 1;
    v.draft.start_time = Date.now() + 20 * 60_000;
    render(<ClockBanner view={v} />);
    expect(screen.getByText(/Draft has not started/)).toHaveTextContent(
      `starts ${localTime(v.draft.start_time)}`,
    );
  });

  it("shows no clock when the draft has none", () => {
    const v = view();
    v.draft.status = "drafting";
    v.draft.pick_deadline = null;
    render(<ClockBanner view={v} />);
    expect(screen.queryByLabelText("Pick clock")).not.toBeInTheDocument();
  });
});

describe("SidePanel tier alerts", () => {
  // Dogfood pass 2, ISSUE-P2-002: after the tier-banding fix the numbers run
  // past 14, and a bare "Tier 7" next to "Tier 1" reads like a ranking across
  // positions rather than "the best band this position has left".
  it("labels the alert as the top band and keeps the number as detail", () => {
    const v = view();
    v.tier_alerts = [
      { position: "RB", tier: 7, players_left: 2 },
      { position: "DEF", tier: 1, players_left: 3 },
    ];
    render(<SidePanel view={v} />);
    const rb = screen.getByRole("listitem", { name: /RB/ });
    expect(rb).toHaveTextContent("Top tier");
    expect(rb).toHaveTextContent("T7");
    expect(rb).toHaveTextContent("2 left");
    expect(rb).toHaveAccessibleDescription(/best RB band still on the board/i);
  });
});

describe("SidePanel structure", () => {
  // Dogfood ISSUE-010: the document jumped from h1 straight to h3.
  it("uses second-level headings under the page title", () => {
    render(<SidePanel view={view()} />);
    for (const name of ["My roster", "Tier alerts", "Recent picks"]) {
      expect(screen.getByRole("heading", { level: 2, name })).toBeInTheDocument();
    }
  });
});

describe("SidePanel recent picks", () => {
  it("names the manager who made each pick, falling back to the slot", () => {
    const v = view();
    v.recent_picks = [
      { pick_no: 26, round: 2, slot: 3, slot_name: "adaigle", player_id: "1", name: "Rashee Rice", position: "WR" },
      { pick_no: 25, round: 2, slot: 4, slot_name: null, player_id: "2", name: "Nico Collins", position: "WR" },
    ];
    render(<SidePanel view={v} />);
    const items = screen.getAllByRole("listitem").map((li) => li.textContent);
    expect(items.find((t) => t?.includes("Rashee Rice"))).toContain("adaigle");
    expect(items.find((t) => t?.includes("Rashee Rice"))).not.toContain("slot 3");
    expect(items.find((t) => t?.includes("Nico Collins"))).toContain("slot 4");
  });
});

describe("ClockBanner accessibility", () => {
  it("announces the draft status politely", () => {
    const v = view();
    v.draft.is_my_pick = true;
    v.draft.pick_deadline = null;
    render(<ClockBanner view={v} />);
    const live = screen.getByRole("status");
    expect(live).toHaveAttribute("aria-live", "polite");
    expect(live).toHaveTextContent("YOU ARE ON THE CLOCK");
  });

  // Dogfood ISSUE-006: the countdown sat inside the live region, so a screen
  // reader re-read the whole banner once a second for the whole draft and
  // buried the one announcement that matters.
  it("keeps the ticking countdown out of the live region", () => {
    vi.useFakeTimers();
    const v = view();
    v.draft.status = "drafting";
    v.draft.is_my_pick = true;
    v.draft.pick_deadline = Date.now() + 47_000;
    render(<ClockBanner view={v} />);

    const live = screen.getByRole("status");
    expect(within(live).queryByLabelText("Pick clock")).not.toBeInTheDocument();
    expect(live).not.toHaveTextContent("0:47");
    // …and the clock is still there to be read on demand.
    expect(screen.getByLabelText("Pick clock")).toHaveTextContent("0:47");

    const announced = live.textContent;
    act(() => vi.advanceTimersByTime(3_000));
    expect(screen.getByLabelText("Pick clock")).toHaveTextContent("0:44");
    expect(live.textContent).toBe(announced);
    vi.useRealTimers();
  });
});

describe("ClockBanner start time", () => {
  // Dogfood ISSUE-012: a start time that had already passed still read
  // "starts 5:00 PM", which on draft day is exactly when it misleads.
  it("says the draft is late once its start time has passed", () => {
    const v = view();
    v.draft.status = "pre_draft";
    v.draft.total_picks_made = 0;
    v.draft.current_pick = 1;
    v.draft.start_time = Date.now() - 20 * 60_000;
    render(<ClockBanner view={v} />);
    const status = screen.getByText(/Draft has not started/);
    expect(status).toHaveTextContent(/scheduled/i);
    expect(status).not.toHaveTextContent(/starts /);
  });

  it("still shows a future start time as a start time", () => {
    const v = view();
    v.draft.status = "pre_draft";
    v.draft.total_picks_made = 0;
    v.draft.current_pick = 1;
    v.draft.start_time = Date.now() + 30 * 60_000;
    render(<ClockBanner view={v} />);
    expect(screen.getByText(/Draft has not started/)).toHaveTextContent(/starts /);
  });
});

describe("SidePanel keepers", () => {
  // The 2026 feed flagged only 24 of 27 keepers, so the backend decides
  // keeper-ness by position and sends `is_keeper`; the panel just says so.
  it("tags kept players on the roster and leaves drafted ones alone", () => {
    const v = view();
    const roster = v.my_roster!;
    roster.players = roster.players.slice(0, 2);
    roster.players[0].is_keeper = true;
    roster.players[1].is_keeper = false;
    render(<SidePanel view={v} />);
    const list = screen.getByRole("list", { name: "My roster" });
    const tags = within(list).getAllByText("keeper");
    expect(tags).toHaveLength(1);
    expect(tags[0].closest("li")).toHaveTextContent(roster.players[0].name);
  });
});
