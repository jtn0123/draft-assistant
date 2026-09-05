// What the parsed Markdown actually becomes on screen.
//
// `chatMarkdown.test.ts` covers the grammar as data; this covers the elements
// it is set as — and above all the one rule the app cannot afford to lose: a
// link in an answer must never be a real link.

import { render, screen } from "@testing-library/react";
import { describe, expect, it } from "vitest";
import { Markdown } from "./Markdown";

describe("Markdown", () => {
  /** Every anchor anywhere in the rendered output. */
  function anchors(container: HTMLElement): HTMLAnchorElement[] {
    return Array.from(container.querySelectorAll("a"));
  }

  /**
   * A link is set as its title with the address on hover. There is nothing in
   * this app that can hand a URL to the real browser, so an `<a href>` inside
   * a chat answer navigates the webview itself: the draft screen is replaced
   * by whatever the model linked to, with the board, the poll loop and the
   * unsaved manual picks going with it. A model that answers with a source
   * link is ordinary, so this is a rule about everyday output, not a corner.
   */
  it("sets a link as a titled span and never as an anchor that could navigate", () => {
    const { container } = render(
      <Markdown text="See [the projections](https://example.com/proj) for more." />,
    );
    const link = screen.getByText("the projections");
    expect(link.tagName).toBe("SPAN");
    expect(link).toHaveClass("chat-md-link");
    // The address is still readable, just not followable.
    expect(link).toHaveAttribute("title", "https://example.com/proj");
    expect(anchors(container)).toHaveLength(0);
    // The text either side of the link survives.
    expect(container.textContent).toContain("See ");
    expect(container.textContent).toContain(" for more.");
  });

  it("keeps links unfollowable inside headings and list items too", () => {
    const { container } = render(
      <Markdown
        text={[
          "# A [heading link](https://example.com/h)",
          "",
          "- A [bullet link](https://example.com/b)",
          "",
          "1. A [numbered link](https://example.com/n)",
        ].join("\n")}
      />,
    );
    for (const [text, url] of [
      ["heading link", "https://example.com/h"],
      ["bullet link", "https://example.com/b"],
      ["numbered link", "https://example.com/n"],
    ]) {
      const link = screen.getByText(text);
      expect(link.tagName).toBe("SPAN");
      expect(link).toHaveAttribute("title", url);
    }
    expect(anchors(container)).toHaveLength(0);
  });

  /**
   * Raw HTML in an answer is text, never markup. A model quoting a page back
   * could otherwise put an anchor — or a script tag — straight into the panel.
   */
  it("shows html in an answer as the text it is", () => {
    const { container } = render(
      <Markdown text={'<a href="https://example.com">click</a><script>alert(1)</script>'} />,
    );
    expect(anchors(container)).toHaveLength(0);
    expect(container.querySelector("script")).toBeNull();
    expect(container.textContent).toContain('<a href="https://example.com">click</a>');
  });

  it("sets each block as the element it reads as", () => {
    const { container } = render(
      <Markdown
        text={[
          "## Who to take",
          "",
          "**Bowers** is the *value* here, per `adp_ppr`.",
          "",
          "- one",
          "- two",
          "",
          "1. first",
          "2. second",
        ].join("\n")}
      />,
    );
    expect(screen.getByRole("heading", { name: "Who to take" }).tagName).toBe("H4");
    expect(container.querySelector("strong")).toHaveTextContent("Bowers");
    expect(container.querySelector("em")).toHaveTextContent("value");
    expect(container.querySelector("code")).toHaveTextContent("adp_ppr");
    expect(container.querySelectorAll("ul > li")).toHaveLength(2);
    expect(container.querySelectorAll("ol > li")).toHaveLength(2);
  });

  /**
   * Answers lay out short "Player — reason" lines. Joined into one run they
   * ran together into a wall of text, so a wrapped paragraph keeps its breaks.
   */
  it("keeps the line breaks inside a paragraph", () => {
    const { container } = render(<Markdown text={"Bowers — value\nOdunze — upside"} />);
    const paragraph = container.querySelector("p");
    expect(paragraph?.textContent).toBe("Bowers — value\nOdunze — upside");
  });

  it("renders nothing rather than failing when there is nothing to render", () => {
    const { container } = render(<Markdown text="" />);
    expect(container.querySelector(".chat-md")?.children).toHaveLength(0);
  });
});
