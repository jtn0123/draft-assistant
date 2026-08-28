import { render, screen } from "@testing-library/react";
import { describe, expect, it } from "vitest";
import { Markdown } from "./Markdown";
import { parseBlocks } from "./chatMarkdown";

describe("Markdown", () => {
  it("renders bold, italic and code inline", () => {
    const { container } = render(
      <Markdown text="Take **Jahmyr Gibbs** — *elite* RB1 under `adp_ppr`." />,
    );
    expect(container.querySelector("strong")).toHaveTextContent("Jahmyr Gibbs");
    expect(container.querySelector("em")).toHaveTextContent("elite");
    expect(container.querySelector("code")).toHaveTextContent("adp_ppr");
    // No stray markers are left behind.
    expect(container.textContent).toBe("Take Jahmyr Gibbs — elite RB1 under adp_ppr.");
  });

  it("renders bullet and numbered lists", () => {
    render(
      <Markdown
        text={"Plan:\n- Gibbs at 2\n- WR at 27\n\n1. Olave\n2. Nabers\n3) London"}
      />,
    );
    const lists = screen.getAllByRole("list");
    expect(lists).toHaveLength(2);
    expect(lists[0].tagName).toBe("UL");
    expect(lists[1].tagName).toBe("OL");
    expect(screen.getAllByRole("listitem").map((li) => li.textContent)).toEqual([
      "Gibbs at 2",
      "WR at 27",
      "Olave",
      "Nabers",
      "London",
    ]);
  });

  it("renders short headings and keeps paragraph breaks", () => {
    const { container } = render(<Markdown text={"### Next three picks\nLine one\nline two\n\nSecond paragraph"} />);
    expect(container.querySelector("h4")).toHaveTextContent("Next three picks");
    const paragraphs = container.querySelectorAll("p");
    expect(paragraphs).toHaveLength(2);
    expect(paragraphs[0].textContent).toBe("Line one\nline two");
    expect(paragraphs[1].textContent).toBe("Second paragraph");
  });

  it("never interprets HTML", () => {
    const { container } = render(<Markdown text={'<script>alert(1)</script> <b>x</b>'} />);
    expect(container.querySelector("script")).toBeNull();
    expect(container.querySelector("b")).toBeNull();
    expect(container.textContent).toBe("<script>alert(1)</script> <b>x</b>");
  });

  it("leaves plain prose and underscores alone", () => {
    expect(parseBlocks("Take Gibbs. my_roster is thin at WR.")).toEqual([
      { kind: "p", lines: ["Take Gibbs. my_roster is thin at WR."] },
    ]);
    const { container } = render(<Markdown text="my_roster is thin_at WR" />);
    expect(container.querySelector("em")).toBeNull();
    expect(container.textContent).toBe("my_roster is thin_at WR");
  });
});
