import { render, screen } from "@testing-library/react";
import { describe, expect, it } from "vitest";
import fixtureJson from "../../public/dev-fixture.json";
import type { DraftView } from "../types";
import { History } from "./History";

function view(): DraftView {
  return structuredClone(fixtureJson) as unknown as DraftView;
}

describe("History", () => {
  it("renders nothing without a previous season", () => {
    const v = view();
    v.history = null;
    const { container } = render(<History view={v} />);
    expect(container).toBeEmptyDOMElement();
  });

  it("summarises the bids and lists each manager", () => {
    const v = view();
    v.history = {
      league_id: "prev",
      trades: 38,
      claims: 142,
      bids: { count: 120, median: 21, p75: 60, max: 400 },
      managers: [
        { user_id: "a", display_name: "ocrevo", trades: 13, moves: 38, faab_used: 696, wins: 14, losses: 14, points_for: 1644 },
        { user_id: "b", display_name: null, trades: 23, moves: 74, faab_used: 1000, wins: 19, losses: 9, points_for: 1817.3 },
      ],
    };
    render(<History view={v} />);
    expect(screen.getByText(/38 trades · 142 claims · winning bid median \$21, top quarter \$60\+/)).toBeInTheDocument();
    const rows = screen.getAllByRole("listitem");
    expect(rows[0]).toHaveTextContent("ocrevo");
    expect(rows[0]).toHaveTextContent("13 tr");
    expect(rows[0]).toHaveTextContent("$696");
    expect(rows[1]).toHaveTextContent("(left the league)");
  });
});
