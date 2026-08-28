import { parseBlocks, renderInline } from "./chatMarkdown";

/** A chat answer's Markdown, rendered as React elements (see `chatMarkdown.tsx`). */
export function Markdown({ text }: { text: string }) {
  const blocks = parseBlocks(text);
  return (
    <div className="chat-md">
      {blocks.map((block, i) => {
        switch (block.kind) {
          case "h":
            return <h4 key={i}>{renderInline(block.text)}</h4>;
          case "ul":
            return (
              <ul key={i}>
                {block.items.map((item, j) => (
                  <li key={j}>{renderInline(item)}</li>
                ))}
              </ul>
            );
          case "ol":
            return (
              <ol key={i}>
                {block.items.map((item, j) => (
                  <li key={j}>{renderInline(item)}</li>
                ))}
              </ol>
            );
          default:
            return (
              <p key={i}>
                {block.lines.map((line, j) => (
                  <span key={j}>
                    {j > 0 && "\n"}
                    {renderInline(line)}
                  </span>
                ))}
              </p>
            );
        }
      })}
    </div>
  );
}
