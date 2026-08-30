import { act, render, screen } from "@testing-library/react";
import { afterEach, beforeEach, describe, expect, it, vi } from "vitest";
import fixtureJson from "../../public/dev-fixture.json";
import type { DraftView } from "../types";
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
