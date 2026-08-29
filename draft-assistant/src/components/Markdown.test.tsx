import { render, screen } from "@testing-library/react";
import { describe, expect, it, vi } from "vitest";
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

describe("links", () => {
  const url =
    "https://www.fantasypros.com/2026/08/nfl-week-1-running-back-usage-report-training-camp-notes/";

  it("shows a bare URL as its site, not its whole address", () => {
    const { container } = render(<Markdown text={`Camp notes: ${url}`} />);
    const link = container.querySelector("a");
    expect(link).toHaveTextContent("fantasypros.com");
    expect(link).toHaveAttribute("href", url);
    // The full address is still reachable, just not printed.
    expect(link).toHaveAttribute("title", url);
    expect(container.textContent).toBe("Camp notes: fantasypros.com");
  });

  it("keeps a titled link's own words", () => {
    const { container } = render(<Markdown text={`See [the usage report](${url}) first.`} />);
    expect(container.querySelector("a")).toHaveTextContent("the usage report");
    expect(container.textContent).toBe("See the usage report first.");
  });

  it("leaves the sentence's full stop out of the link", () => {
    const { container } = render(<Markdown text="Per https://espn.com/nfl/story." />);
    expect(container.querySelector("a")).toHaveAttribute("href", "https://espn.com/nfl/story");
    expect(container.textContent).toBe("Per espn.com.");
  });

  it("does not make a link out of anything that is not http", () => {
    const { container } = render(<Markdown text="Run `javascript:alert(1)` never." />);
    expect(container.querySelector("a")).toBeNull();
  });

  it("hands the click to the browser instead of navigating this window", () => {
    // jsdom has no window.open, and outside Tauri that is the path taken.
    const open = vi.spyOn(window, "open").mockReturnValue(null);
    const { container } = render(<Markdown text={url} />);
    const event = new MouseEvent("click", { bubbles: true, cancelable: true });
    container.querySelector("a")?.dispatchEvent(event);
    // The draft is in this window: following the href here would end it.
    expect(event.defaultPrevented).toBe(true);
    expect(open).toHaveBeenCalledWith(url, "_blank", "noopener,noreferrer");
    open.mockRestore();
  });
});
