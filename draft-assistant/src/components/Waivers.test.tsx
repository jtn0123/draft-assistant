import { render, screen } from "@testing-library/react";
import { describe, expect, it } from "vitest";
import fixtureJson from "../../public/dev-fixture.json";
import type { DraftView, WaiverTarget } from "../types";
import { Waivers } from "./Waivers";

function view(): DraftView {
  return structuredClone(fixtureJson) as unknown as DraftView;
}
function target(name: string, pos: string, gain: number, rivals: number, trending: number | null): WaiverTarget {
  return {
    player_id: name,
    name,
    position: pos,
    team: "NYG",
    bye_week: 8,
    points: 98,
    my_gain: gain,
    rivals_helped: rivals,
    trending_adds: trending,
    suggested_bid: null,
  };
}

describe("Waivers", () => {
  it("renders nothing before the draft is over", () => {
    const v = view();
    v.waivers = null;
    const { container } = render(<Waivers view={v} />);
    expect(container).toBeEmptyDOMElement();
  });

  it("lists who would start for me, with the competition and the drop", () => {
    const v = view();
    v.waivers = {
      targets: [
        { ...target("New York Giants", "DEF", 92.3, 1, null), suggested_bid: 60 },
        target("MarShawn Lloyd", "RB", 6.1, 4, 157140),
        target("Jack Bech", "WR", 0, 0, 2000),
      ],
      drops: [{ player_id: "s", name: "Nicholas Singleton", position: "RB", points: 63.7, starts: 0 }],
    };
    render(<Waivers view={v} />);
    // The bye is on the row, not only in a hover title.
    expect(screen.getAllByRole("listitem")[0]).toHaveTextContent("bye 8");
    const rows = screen.getAllByRole("listitem");
    // A zero-gain player is not a target, however hot he is elsewhere.
    expect(rows).toHaveLength(2);
    expect(rows[0]).toHaveTextContent("New York Giants");
    expect(rows[0]).toHaveTextContent("+92");
    expect(rows[0]).toHaveTextContent("1 rival");
    expect(rows[0]).toHaveTextContent("$60");
    expect(rows[1]).toHaveTextContent("4 rivals");
    expect(rows[1]).toHaveTextContent("🔥157k");
    expect(screen.getByText(/Drop first:/)).toHaveTextContent("Nicholas Singleton (RB, never starts)");
  });
});
