import { describe, expect, it } from "vitest";
import { inlineTokens, parseBlocks } from "./chatMarkdown";

describe("blocks", () => {
  it("groups consecutive lines into a paragraph and splits on a blank line", () => {
    expect(parseBlocks("one\ntwo\n\nthree")).toEqual([
      { kind: "p", lines: ["one", "two"] },
      { kind: "p", lines: ["three"] },
    ]);
  });

  it("collects bullets and numbers into one list each", () => {
    expect(parseBlocks("- a\n- b\n\n1. first\n2) second")).toEqual([
      { kind: "ul", items: ["a", "b"] },
      { kind: "ol", items: ["first", "second"] },
    ]);
  });

  it("reads a heading", () => {
    expect(parseBlocks("## Take the RB")).toEqual([{ kind: "h", text: "Take the RB" }]);
  });

  it("keeps carriage returns and plain text intact", () => {
    expect(parseBlocks("a\r\nb")).toEqual([{ kind: "p", lines: ["a", "b"] }]);
    expect(parseBlocks("<script>alert(1)</script>")).toEqual([
      { kind: "p", lines: ["<script>alert(1)</script>"] },
    ]);
  });
});

describe("inline runs", () => {
  it("marks bold, italic and code", () => {
    expect(inlineTokens("take **Bijan** now, *maybe* `adp_ppr`")).toEqual([
      { kind: "text", text: "take " },
      { kind: "bold", text: "Bijan" },
      { kind: "text", text: " now, " },
      { kind: "italic", text: "maybe" },
      { kind: "text", text: " " },
      { kind: "code", text: "adp_ppr" },
    ]);
  });

  it("reads bold before italic so **x** is never two italics", () => {
    expect(inlineTokens("**x**")).toEqual([{ kind: "bold", text: "x" }]);
  });

  it("leaves underscores alone — player keys are not italics", () => {
    expect(inlineTokens("my_roster and adp_ppr")).toEqual([
      { kind: "text", text: "my_roster and adp_ppr" },
    ]);
  });

  it("keeps a link's title and where it pointed", () => {
    expect(inlineTokens("see [the report](https://example.com/x)")).toEqual([
      { kind: "text", text: "see " },
      { kind: "link", text: "the report", url: "https://example.com/x" },
    ]);
  });

  it("passes text with no markup straight through", () => {
    expect(inlineTokens("plain words")).toEqual([{ kind: "text", text: "plain words" }]);
  });
});
