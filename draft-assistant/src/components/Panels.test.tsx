import { act, render, screen } from "@testing-library/react";
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
