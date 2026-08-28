import { act, render, screen, within } from "@testing-library/react";
import { afterEach, beforeEach, describe, expect, it, vi } from "vitest";
import fixtureJson from "../../public/dev-fixture.json";
import type { DraftView } from "../types";
import { ClockBanner, SidePanel } from "./Panels";

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
    v.draft.start_time = new Date("2026-08-28T17:30:00-07:00").getTime();
    render(<ClockBanner view={v} />);
    expect(screen.getByText(/Draft has not started/)).toHaveTextContent(/starts .*5:30/);
  });

  it("shows no clock when the draft has none", () => {
    const v = view();
    v.draft.status = "drafting";
    v.draft.pick_deadline = null;
    render(<ClockBanner view={v} />);
    expect(screen.queryByLabelText("Pick clock")).not.toBeInTheDocument();
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
    v.draft.start_time = Date.now() + 30 * 60_000;
    render(<ClockBanner view={v} />);
    expect(screen.getByText(/Draft has not started/)).toHaveTextContent(/starts /);
  });
});
