import { render, screen } from "@testing-library/react";
import { describe, expect, it } from "vitest";
import fixtureJson from "../../public/dev-fixture.json";
import type { DraftView } from "../types";
import { ByeWeeks } from "./ByeWeeks";

function view(): DraftView {
  return structuredClone(fixtureJson) as unknown as DraftView;
}

describe("ByeWeeks", () => {
  it("renders nothing without a roster", () => {
    const v = view();
    v.bye_weeks = [];
    const { container } = render(<ByeWeeks view={v} />);
    expect(container).toBeEmptyDOMElement();
  });

  it("names the worst week up front and marks an empty slot", () => {
    const v = view();
    v.bye_weeks = [
      { week: 5, out: ["Bijan Robinson", "Sam LaPorta"], points: 98.1, shortfall: 21.4, empty_slots: ["TE"] },
      { week: 8, out: ["Tee Higgins"], points: 112.0, shortfall: 7.5, empty_slots: [] },
    ];
    render(<ByeWeeks view={v} />);
    expect(screen.getByText(/worst: week 5, −21\.4 \(TE empty\)/)).toBeInTheDocument();
    const rows = screen.getAllByRole("listitem");
    expect(rows[0]).toHaveClass("empty");
    expect(rows[0]).toHaveTextContent("Bijan Robinson, Sam LaPorta");
    expect(rows[1]).not.toHaveClass("empty");
    expect(rows[1]).toHaveTextContent("−7.5");
  });
});
