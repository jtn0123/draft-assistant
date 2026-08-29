import { render, screen } from "@testing-library/react";
import { describe, expect, it } from "vitest";
import fixtureJson from "../../public/dev-fixture.json";
import type { DraftView, Starter } from "../types";
import { ThisWeek } from "./ThisWeek";

function view(): DraftView {
  return structuredClone(fixtureJson) as unknown as DraftView;
}
const st = (slot: string, name: string, points: number): Starter => ({
  slot,
  player_id: name,
  name,
  position: slot,
  points,
  injury: null,
});

describe("ThisWeek", () => {
  it("renders nothing without a week", () => {
    const v = view();
    v.this_week = null;
    const { container } = render(<ThisWeek view={v} />);
    expect(container).toBeEmptyDOMElement();
  });

  it("says what to change, marks an empty slot, and shows the matchup odds", () => {
    const v = view();
    v.this_week = {
      week: 1,
      lineup: {
        set_points: 119.3,
        best_points: 125.5,
        changes: [
          { slot: "FLEX", out: st("RB", "Kenny Gainwell", 9.4), in_: st("WR", "Khalil Shakir", 11.5), gain: 2.1 },
          { slot: "DEF", out: null, in_: st("DEF", "New York Giants", 6.2), gain: 6.2 },
          { slot: "WR", out: { ...st("WR", "Tee Higgins", 0), injury: "Out" }, in_: st("WR", "Michael Wilson", 11.6), gain: 11.6 },
        ],
        empty_slots: ["DEF"], questionable: [],
      },
      matchup: {
        opponent_slot: 6,
        opponent_name: "MeatballMike09",
        my_points: 125.5,
        opponent_points: 125.4,
        margin: 0.1,
        win_probability: 0.5013,
        my_starters: [st("QB", "Matthew Stafford", 20.7)],
        opponent_starters: [st("QB", "Kyler Murray", 20.1)],
      },
    };
    render(<ThisWeek view={v} />);
    expect(screen.getByRole("heading", { name: "Lineup check" })).toBeInTheDocument();
    const rows = screen.getAllByRole("listitem");
    expect(rows[0]).toHaveTextContent("Khalil Shakir (11.5) over Kenny Gainwell (9.4)");
    expect(rows[0]).toHaveTextContent("+2.1");
    expect(rows[1]).toHaveTextContent("empty — start New York Giants");
    expect(rows[1]).toHaveClass("empty");
    expect(rows[2]).toHaveTextContent("Tee Higgins is Out — start Michael Wilson (11.6)");
    expect(screen.getByText("MeatballMike09")).toBeInTheDocument();
    expect(screen.getByText("125.5 – 125.4")).toBeInTheDocument();
    expect(screen.getByText("50% to win")).toBeInTheDocument();
  });

  it("says so when the set lineup is already the best", () => {
    const v = view();
    v.this_week = {
      week: 3,
      lineup: { set_points: 120, best_points: 120, changes: [], empty_slots: [], questionable: [] },
      matchup: null,
    };
    render(<ThisWeek view={v} />);
    expect(screen.getByText(/is the best one/)).toBeInTheDocument();
  });
});

describe("ThisWeek with a slot nobody can fill", () => {
  it("names the empty slot without pretending there is a swap to make", () => {
    const v = view();
    v.this_week = {
      week: 1,
      lineup: { set_points: 119.3, best_points: 119.3, changes: [], empty_slots: ["DEF"], questionable: [] },
      matchup: null,
    };
    render(<ThisWeek view={v} />);
    expect(screen.getByText(/Best lineup set — 119.3 projected/)).toBeInTheDocument();
    expect(screen.getByText(/DEF empty and nobody on the roster fills it/)).toBeInTheDocument();
    expect(screen.queryByText(/\+0\.0/)).not.toBeInTheDocument();
  });
});

describe("ThisWeek questionable starters", () => {
  it("lists set starters to check before kickoff", () => {
    const v = view();
    v.this_week = {
      week: 1,
      lineup: {
        set_points: 119.3,
        best_points: 119.3,
        changes: [],
        empty_slots: [],
        questionable: [{ ...st("TE", "Sam LaPorta", 12.2), injury: "Questionable" }],
      },
      matchup: null,
    };
    render(<ThisWeek view={v} />);
    expect(screen.getByText(/Check before kickoff/)).toHaveTextContent("Sam LaPorta (Questionable)");
  });
});
