import type { ReactNode } from "react";
import { isSafeUrl, openExternal } from "../openExternal";

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

// Links, then bold, code, italic — bold before italic so `**x**` is never
// read as two italics, and links before everything so the URL inside
// `[title](…)` is not also matched on its own. Underscore italics are
// deliberately not supported: `adp_ppr` and `my_roster` appear in answers and
// would be mangled.
const INLINE =
  /(\[[^\]\n]+\]\(https?:\/\/[^\s)]+\)|https?:\/\/[^\s<>"'`)\]]+|\*\*[^*\n]+\*\*|`[^`\n]+`|\*[^*\n]+\*)/g;

const MD_LINK = /^\[([^\]\n]+)\]\((https?:\/\/[^\s)]+)\)$/;
const IS_URL = /^https?:\/\//;
/** Sentence punctuation that trailed the URL rather than belonging to it. */
const TRAILING = /[.,;:!?]+$/;

/**
 * What to show for a link with no title of its own: the site, not the whole
 * address. A search result URL runs to a couple of hundred characters, wraps
 * over four lines of a panel this narrow, and says nothing the sentence around
 * it did not already say.
 */
export function linkLabel(url: string): string {
  try {
    return new URL(url).hostname.replace(/^www\./, "");
  } catch {
    return url;
  }
}

/** Opened in the real browser, never here — see `openExternal`. */
function anchor(url: string, text: string, key: number): ReactNode {
  if (!isSafeUrl(url)) return text;
  return (
    <a
      key={key}
      className="chat-link"
      href={url}
      title={url}
      onClick={(e) => {
        e.preventDefault();
        void openExternal(url);
      }}
    >
      {text}
    </a>
  );
}

export function renderInline(text: string): ReactNode[] {
  const out: ReactNode[] = [];
  let last = 0;
  let key = 0;
  for (const match of text.matchAll(INLINE)) {
    const start = match.index ?? 0;
    if (start > last) out.push(text.slice(last, start));
    const token = match[0];
    const link = MD_LINK.exec(token);
    if (link) {
      out.push(anchor(link[2], IS_URL.test(link[1]) ? linkLabel(link[1]) : link[1], key++));
    } else if (IS_URL.test(token)) {
      const url = token.replace(TRAILING, "");
      out.push(anchor(url, linkLabel(url), key++));
      // The full stop that ended the sentence stays in the sentence.
      if (url.length < token.length) out.push(token.slice(url.length));
    } else if (token.startsWith("**")) {
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
