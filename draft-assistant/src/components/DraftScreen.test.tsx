import { render, screen } from "@testing-library/react";
import { afterEach, describe, expect, it, vi } from "vitest";
import fixtureJson from "../../public/dev-fixture.json";
import type { DraftView } from "../types";
import { stableAvailable } from "../boardIdentity";
import { DraftScreen } from "./DraftScreen";

/** The fixture, trimmed to a pool small enough to render quickly. */
function fixture(): DraftView {
  const view = structuredClone(fixtureJson) as unknown as DraftView;
  view.draft.status = "drafting";
  view.available = view.available.slice(0, 5);
  return view;
}

const screenFor = (view: DraftView) => <DraftScreen view={view} busy={false} onDraft={vi.fn()} />;

describe("DraftScreen", () => {
  afterEach(() => vi.restoreAllMocks());

  it("shows a player once even when two modes recommend them", () => {
    const view = fixture();
    // The fixture's balanced and safe modes both land on Chris Olave.
    expect(view.recommendations).toHaveLength(2);
    expect(new Set(view.recommendations.map((r) => r.player_id)).size).toBe(1);

    const { container } = render(screenFor(view));
    expect(container.querySelectorAll(".rec")).toHaveLength(1);
    // …carrying the position rank the pool gives that player.
    expect(screen.getByText("WR11")).toBeInTheDocument();
  });

  // Grade item G7. The rank of each recommended player used to be a full scan
  // of the pool per card, on every render. It is one pass now, and `applyView`
  // recycles the pool's array whenever an update says nothing new about it
  // (boardIdentity.ts), so the pass is skipped entirely on those updates.
  it("does not walk the pool again when an update leaves it alone", () => {
    const view = fixture();
    const { rerender } = render(screenFor(view));

    // The only thing that reads the pool as a whole is the rank map.
    const scans = vi.spyOn(view.available, "map");
    const tick = stableAvailable(view, {
      ...view,
      available: view.available.map((p) => ({ ...p })),
    });
    expect(tick.available).toBe(view.available);
    scans.mockClear();

    rerender(screenFor(tick));
    expect(scans).not.toHaveBeenCalled();
    expect(screen.getByText("WR11")).toBeInTheDocument();
  });

  it("shows the new rank when the pool genuinely changes", () => {
    const view = fixture();
    const { rerender } = render(screenFor(view));
    expect(screen.getByText("WR11")).toBeInTheDocument();

    const moved = stableAvailable(view, {
      ...view,
      available: view.available.map((p, i) => (i === 0 ? { ...p, position_rank: 4 } : p)),
    });
    expect(moved.available).not.toBe(view.available);
    rerender(screenFor(moved));
    expect(screen.getByText("WR4")).toBeInTheDocument();
    expect(screen.queryByText("WR11")).not.toBeInTheDocument();
  });
});
