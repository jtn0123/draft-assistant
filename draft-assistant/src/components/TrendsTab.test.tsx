import { fireEvent, render, screen } from "@testing-library/react";
import { beforeEach, describe, expect, it, vi } from "vitest";

const mocks = vi.hoisted(() => ({ avatar: vi.fn() }));
vi.mock("../api", () => ({ api: mocks }));

import type { TrendsView } from "../season-types";
import { resetAvatarCache } from "../avatars";
import { dateLabel } from "../format";
import { TrendsTab } from "./TrendsTab";
import { settle } from "../test/settle";

beforeEach(() => {
  vi.clearAllMocks();
  resetAvatarCache();
  mocks.avatar.mockResolvedValue("data:image/png;base64,AAAA");
});

const T0 = Date.parse("2026-09-03T14:00:00Z") / 1000;
const DAY = 86_400;

function view(snapshots: number): TrendsView {
  const points = (base: number, step: number) =>
    Array.from({ length: snapshots }, (_, i) => ({
      at: T0 + i * DAY,
      week: 1 + i,
      strength: base + i * step,
    }));
  return {
    series: [
      { roster_id: 2, name: "Witzy's Blitzys", is_mine: true, points: points(130, 2) },
      { roster_id: 7, name: "Mitch's Cousin", is_mine: false, points: points(110, -1) },
    ],
    changes:
      snapshots > 1
        ? [
            {
              at: T0 + DAY,
              week: 2,
              roster_id: 7,
              team: "Mitch's Cousin",
              is_mine: false,
              delta: -4.1,
              reasons: ["traded CeeDee Lamb for Travis Etienne", "Josh Allen now Q (−2.0/wk)"],
            },
          ]
        : [],
  };
}

describe("TrendsTab", () => {
  it("explains that the graph needs a second snapshot", () => {
    render(<TrendsTab trends={view(1)} />);
    expect(screen.getByText(/first snapshot taken/)).toBeInTheDocument();
    expect(screen.queryByRole("img")).not.toBeInTheDocument();
    expect(screen.getByText(/Nothing has moved yet/)).toBeInTheDocument();
  });

  it("shows where everyone stands until there are enough readings to plot", () => {
    const { container } = render(<TrendsTab trends={view(2)} />);
    expect(screen.queryByRole("img")).not.toBeInTheDocument();
    expect(screen.getByText(/Where everyone stands/)).toBeInTheDocument();
    // Strongest team first, mine marked out, each with its move so far.
    const rows = [...container.querySelectorAll(".trend-rank-row")];
    expect(rows).toHaveLength(2);
    expect(rows[0].textContent).toContain("Witzy's Blitzys");
    expect(rows[0].className).toContain("is-mine");
    expect(rows[0].textContent).toContain("+2.0");
    expect(rows[1].textContent).toContain("−1.0");
    // A team that moved gets a tail back to where it started.
    expect(rows[0].querySelector(".trend-tail")).not.toBeNull();
  });

  it("leaves the tail off a team that has not moved", () => {
    const flat = view(2);
    for (const s of flat.series) s.points[1].strength = s.points[0].strength;
    const { container } = render(<TrendsTab trends={flat} />);
    const rows = [...container.querySelectorAll(".trend-rank-row")];
    expect(rows.every((r) => r.querySelector(".trend-tail") === null)).toBe(true);
    expect(container.querySelectorAll(".trend-mark")).toHaveLength(2);
  });

  it("draws one line per team and lists the reasons a team moved", () => {
    render(<TrendsTab trends={view(3)} />);
    const chart = screen.getByRole("img", { name: /Projected strength/ });
    expect(chart.querySelectorAll("path.trend-line")).toHaveLength(2);
    expect(chart.querySelector("path.trend-line.is-mine")).not.toBeNull();
    expect(screen.getByText("−4.1 / wk")).toBeInTheDocument();
    expect(
      screen.getByText("traded CeeDee Lamb for Travis Etienne · Josh Allen now Q (−2.0/wk)"),
    ).toBeInTheDocument();
  });

  it("shows each team's change since the first snapshot in the legend", () => {
    render(<TrendsTab trends={view(3)} />);
    expect(screen.getByText("+4.0")).toBeInTheDocument();
    expect(screen.getByText("−2.0")).toBeInTheDocument();
  });

  it("em-dashes the movement column while there is only one reading", () => {
    const { container } = render(<TrendsTab trends={view(1)} />);
    const moves = [...container.querySelectorAll(".trend-rank-row")].map(
      (r) => r.lastElementChild?.textContent,
    );
    // Every team is its own baseline on the first snapshot, so "0.0" here
    // would be reporting a measurement nobody has taken yet.
    expect(moves).toEqual(["—", "—"]);
    // The strength column is real data and still prints.
    expect(screen.getByText("130.0")).toBeInTheDocument();
  });

  it("calls sub-tenth drift flat instead of printing a signed zero", () => {
    const flat = view(2);
    flat.series[0].points[1].strength = flat.series[0].points[0].strength - 0.02;
    render(<TrendsTab trends={flat} />);
    expect(screen.getByText("0.0")).toBeInTheDocument();
    expect(screen.queryByText("−0.0")).not.toBeInTheDocument();
  });

  it("highlights a team when its legend row is picked", () => {
    render(<TrendsTab trends={view(3)} />);
    fireEvent.click(screen.getByRole("button", { name: /Mitch's Cousin/ }));
    const chart = screen.getByRole("img", { name: /Projected strength/ });
    expect(chart.querySelector("path.trend-line.is-focus")).not.toBeNull();
  });

  it("hides the chart in table mode but keeps the legend", () => {
    render(<TrendsTab trends={view(3)} />);
    fireEvent.click(screen.getByRole("button", { name: "Table" }));
    expect(screen.queryByRole("img")).not.toBeInTheDocument();
    expect(screen.getByRole("button", { name: /Witzy/ })).toBeInTheDocument();
  });

  it("says nothing has moved once there are readings enough to plot", () => {
    // Five readings, all flat: the chart is withheld because no line would
    // go anywhere, not because the third reading is still to come.
    const flat = view(5);
    for (const s of flat.series) for (const p of s.points) p.strength = s.points[0].strength;
    render(<TrendsTab trends={flat} />);
    expect(screen.getByText(/across 5 readings/)).toBeInTheDocument();
    expect(screen.getByText(/No team has moved enough to plot yet/)).toBeInTheDocument();
    expect(screen.queryByText(/starts from the third/)).not.toBeInTheDocument();
  });

  it("says the chart starts from the third while there are only two readings", () => {
    render(<TrendsTab trends={view(2)} />);
    expect(screen.getByText(/across 2 readings/)).toBeInTheDocument();
    expect(screen.getByText(/The line chart starts from the third/)).toBeInTheDocument();
    expect(screen.queryByText(/No team has moved enough/)).not.toBeInTheDocument();
  });

  it("labels dates in Eastern time", () => {
    expect(dateLabel(Date.parse("2026-09-04T03:30:00Z") / 1000)).toBe("Sep 3");
    expect(dateLabel(Date.parse("2026-09-04T03:30:00Z") / 1000, true)).toBe("Sep 3, 11:30 PM");
  });
});

// Grade item G7. `Math.min(...values)` spread every point of every series into
// one argument list; a league with enough snapshots behind it eventually
// crosses the engine's argument limit and the tab throws instead of drawing.
describe("TrendsTab with a long history", () => {
  const long = (points: number): TrendsView => ({
    series: [2, 7].map((roster_id) => ({
      roster_id,
      name: `Team ${roster_id}`,
      is_mine: roster_id === 2,
      points: Array.from({ length: points }, (_, i) => ({
        at: T0 + i * 60,
        week: 1 + Math.floor(i / 100),
        strength: 100 + (i % 50),
      })),
    })),
    changes: [],
  });

  it("draws a chart from more readings than an argument list can hold", () => {
    // Comfortably past the ~125k spread limit once both series are counted.
    expect(() => render(<TrendsTab trends={long(80_000)} />)).not.toThrow();
    const chart = screen.getByRole("img", { name: /Projected strength/ });
    expect(chart.querySelectorAll("path.trend-line")).toHaveLength(2);
    expect(screen.getByText("80000 snapshots")).toBeInTheDocument();
  });

  it("keeps the same lines while the pointer moves over them", () => {
    const { container } = render(<TrendsTab trends={view(3)} />);
    const chart = screen.getByRole("img", { name: /Projected strength/ });
    const before = [...chart.querySelectorAll("path.trend-line")];

    fireEvent.mouseMove(chart, { clientX: 40 });
    expect(container.querySelector(".trend-crosshair")).not.toBeNull();
    // Hover moves the crosshair; the fourteen path strings behind it are not
    // rebuilt, and React keeps the very same elements.
    const after = [...chart.querySelectorAll("path.trend-line")];
    expect(after).toEqual(before);
    expect(after.map((p) => p.getAttribute("d"))).toEqual(before.map((p) => p.getAttribute("d")));
  });
});

// A legend row is itself a button, so the team picture inside it must not be
// the zoom button: a button inside a button is invalid HTML, and it also put
// the avatar in the flexible column and wrapped the delta onto a second line.
describe("the trends legend row", () => {
  const avatars = { "2": "aaa", "7": "bbb" };

  it("nests no button inside the legend row, in either view", async () => {
    const { container } = render(<TrendsTab trends={view(3)} avatars={avatars} />);
    await settle(() => {});
    expect(container.querySelectorAll("img.team-avatar").length).toBeGreaterThan(0);
    expect(container.querySelectorAll(".trend-legend-row").length).toBeGreaterThan(0);
    expect(container.querySelectorAll("button button")).toHaveLength(0);

    await settle(() => {
      fireEvent.click(screen.getByRole("button", { name: "Table" }));
    });
    expect(container.querySelectorAll(".trend-legend-row").length).toBeGreaterThan(0);
    expect(container.querySelectorAll("button button")).toHaveLength(0);
  });

  it("keeps picture and name in one cell so the row has four children", async () => {
    const { container } = render(<TrendsTab trends={view(3)} avatars={avatars} />);
    await settle(() => {});
    const row = container.querySelector(".trend-legend-row") as HTMLElement;
    // Four children for the four declared grid columns; a fifth would wrap.
    expect(row.children).toHaveLength(4);
    const cell = row.children[1] as HTMLElement;
    expect(cell.className).toContain("team-cell");
    expect(cell.querySelector("img.team-avatar")).not.toBeNull();
    expect(cell.textContent).toContain("Witzy's Blitzys");
  });
});
