// The small slice of Markdown a chat answer actually uses — paragraphs,
// bullet and numbered lists, short headings, **bold**, *italic* and `code`.
//
// Parsing is kept here, away from the elements it becomes (`Markdown.tsx`),
// so the grammar can be tested as data. No HTML is ever interpreted: a
// `<script>` in an answer is text like any other. Anything outside this
// grammar falls through as plain text, so an unexpected answer degrades to
// what the panel showed before Markdown, never to a blank.

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
    const last = blocks[blocks.length - 1];
    if (bullet) {
      flush();
      if (last?.kind === "ul") last.items.push(bullet[1]);
      else blocks.push({ kind: "ul", items: [bullet[1]] });
    } else if (numbered) {
      flush();
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

/** A run of text inside a block, and how it is set. */
export interface InlineToken {
  kind: "text" | "bold" | "italic" | "code" | "link";
  text: string;
  /** Where a link pointed, kept so the panel can show it on hover. */
  url?: string;
}

// Links first, so the URL inside `[title](…)` is not matched again on its
// own; bold before italic, so `**x**` is never read as two italics.
// Underscore italics are deliberately unsupported: `adp_ppr` and `my_roster`
// appear in answers and would be mangled.
const INLINE = /(\[[^\]\n]+\]\(https?:\/\/[^\s)]+\)|\*\*[^*\n]+\*\*|`[^`\n]+`|\*[^*\n]+\*)/g;
const MD_LINK = /^\[([^\]\n]+)\]\((https?:\/\/[^\s)]+)\)$/;

function token(raw: string): InlineToken {
  const link = MD_LINK.exec(raw);
  if (link) return { kind: "link", text: link[1], url: link[2] };
  if (raw.startsWith("**")) return { kind: "bold", text: raw.slice(2, -2) };
  if (raw.startsWith("`")) return { kind: "code", text: raw.slice(1, -1) };
  return { kind: "italic", text: raw.slice(1, -1) };
}

/** One line of a block, split into the runs that are set differently. */
export function inlineTokens(text: string): InlineToken[] {
  const out: InlineToken[] = [];
  let last = 0;
  for (const match of text.matchAll(INLINE)) {
    const start = match.index ?? 0;
    if (start > last) out.push({ kind: "text", text: text.slice(last, start) });
    out.push(token(match[0]));
    last = start + match[0].length;
  }
  if (last < text.length) out.push({ kind: "text", text: text.slice(last) });
  return out;
}
