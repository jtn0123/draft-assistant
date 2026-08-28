import type { ReactNode } from "react";

/**
 * The small slice of Markdown a chat answer actually uses — paragraphs,
 * bullet and numbered lists, short headings, **bold**, *italic* and `code`.
 * No HTML is ever interpreted: a `<script>` in the text is shown as text.
 * Anything outside this grammar falls through as plain text, so an
 * unexpected answer degrades to what the panel showed before, not to a blank.
 */

export type Block =
  | { kind: "p"; lines: string[] }
  | { kind: "h"; text: string }
  | { kind: "ul"; items: string[] }
  | { kind: "ol"; items: string[] };

const BULLET = /^\s*[-*•]\s+(.*)$/;
const NUMBERED = /^\s*\d+[.)]\s+(.*)$/;
const HEADING = /^\s*#{1,4}\s+(.*)$/;

export function parseBlocks(text: string): Block[] {
  const blocks: Block[] = [];
  let paragraph: string[] = [];
  const flush = () => {
    if (paragraph.length > 0) {
      blocks.push({ kind: "p", lines: paragraph });
      paragraph = [];
    }
  };
  for (const raw of text.replace(/\r\n?/g, "\n").split("\n")) {
    const line = raw.trimEnd();
    if (line.trim() === "") {
      flush();
      continue;
    }
    const bullet = BULLET.exec(line);
    const numbered = NUMBERED.exec(line);
    const heading = HEADING.exec(line);
    if (bullet) {
      flush();
      const last = blocks[blocks.length - 1];
      if (last?.kind === "ul") last.items.push(bullet[1]);
      else blocks.push({ kind: "ul", items: [bullet[1]] });
    } else if (numbered) {
      flush();
      const last = blocks[blocks.length - 1];
      if (last?.kind === "ol") last.items.push(numbered[1]);
      else blocks.push({ kind: "ol", items: [numbered[1]] });
    } else if (heading) {
      flush();
      blocks.push({ kind: "h", text: heading[1] });
    } else {
      paragraph.push(line.trim());
    }
  }
  flush();
  return blocks;
}

// Bold, code, then italic — in that order so `**x**` is never read as two
// italics. Underscore italics are deliberately not supported: `adp_ppr` and
// `my_roster` appear in answers and would be mangled.
const INLINE = /(\*\*[^*\n]+\*\*|`[^`\n]+`|\*[^*\n]+\*)/g;

export function renderInline(text: string): ReactNode[] {
  const out: ReactNode[] = [];
  let last = 0;
  let key = 0;
  for (const match of text.matchAll(INLINE)) {
    const start = match.index ?? 0;
    if (start > last) out.push(text.slice(last, start));
    const token = match[0];
    if (token.startsWith("**")) {
      out.push(<strong key={key++}>{token.slice(2, -2)}</strong>);
    } else if (token.startsWith("`")) {
      out.push(<code key={key++}>{token.slice(1, -1)}</code>);
    } else {
      out.push(<em key={key++}>{token.slice(1, -1)}</em>);
    }
    last = start + token.length;
  }
  if (last < text.length) out.push(text.slice(last));
  return out;
}
