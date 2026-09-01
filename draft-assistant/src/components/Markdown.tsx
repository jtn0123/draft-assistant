// A Claude answer, set as the Markdown it was written in. The grammar and
// the parsing live in `chatMarkdown.ts`; this file is only what each piece
// looks like.

import type { ReactNode } from "react";
import { inlineTokens, parseBlocks, type InlineToken } from "../chatMarkdown";

/** A link is shown as its title with the address on hover, not as a link:
 *  this app has no way to hand a URL to the real browser, and a link that
 *  navigates the webview would take the draft down with it. */
function piece(token: InlineToken, key: number): ReactNode {
  switch (token.kind) {
    case "bold":
      return <strong key={key}>{token.text}</strong>;
    case "italic":
      return <em key={key}>{token.text}</em>;
    case "code":
      return <code key={key}>{token.text}</code>;
    case "link":
      return (
        <span className="chat-md-link" key={key} title={token.url}>
          {token.text}
        </span>
      );
    default:
      return token.text;
  }
}

function inline(text: string): ReactNode[] {
  return inlineTokens(text).map(piece);
}

export function Markdown({ text }: { text: string }) {
  return (
    <div className="chat-md">
      {parseBlocks(text).map((block, i) => {
        switch (block.kind) {
          case "h":
            return <h4 key={i}>{inline(block.text)}</h4>;
          case "ul":
            return (
              <ul key={i}>
                {block.items.map((item, j) => (
                  <li key={j}>{inline(item)}</li>
                ))}
              </ul>
            );
          case "ol":
            return (
              <ol key={i}>
                {block.items.map((item, j) => (
                  <li key={j}>{inline(item)}</li>
                ))}
              </ol>
            );
          default:
            // A wrapped paragraph keeps its own line breaks — answers lay out
            // short "Player — reason" lines that must not run together.
            return (
              <p key={i}>
                {block.lines.map((line, j) => (
                  <span key={j}>
                    {j > 0 && "\n"}
                    {inline(line)}
                  </span>
                ))}
              </p>
            );
        }
      })}
    </div>
  );
}
