// The turns of a local conversation, or what stands in for them: the key form
// when there is no key, and the empty-thread note when nothing has been asked.

import { useEffect, useRef, type ReactNode } from "react";
import type { ThreadEntry } from "../chat-types";
import { Markdown } from "./Markdown";

export function ChatThread({
  entries,
  compact,
  sending,
  keyForm,
  emptyTitle,
  emptyNote,
}: {
  entries: ThreadEntry[];
  compact: boolean;
  /** True while an answer is on its way; the thread keeps its end in view. */
  sending: boolean;
  /** Shown in place of the thread while a key is being added. */
  keyForm: ReactNode | null;
  emptyTitle: string;
  emptyNote: string;
}) {
  const threadRef = useRef<HTMLDivElement>(null);

  // Keep the newest turn in view as the thread grows.
  useEffect(() => {
    threadRef.current?.scrollTo({ top: threadRef.current.scrollHeight });
  }, [entries, sending]);

  return (
    <div className={compact ? "chat-thread is-compact" : "chat-thread"} ref={threadRef}>
      {keyForm !== null ? (
        keyForm
      ) : entries.length === 0 ? (
        <div className="chat-empty">
          <span className="chat-empty-title">{emptyTitle}</span>
          <span className="mid small">{emptyNote}</span>
        </div>
      ) : (
        entries.map((entry) => (
          <div className={`msg is-${entry.kind}`} key={entry.id}>
            {entry.label && <span className="msg-label">{entry.label}</span>}
            {entry.kind === "claude" ? (
              <Markdown text={entry.lines.join("\n\n")} />
            ) : (
              entry.lines.map((line, i) => (
                <span className="msg-line" key={i}>
                  {line}
                </span>
              ))
            )}
          </div>
        ))
      )}
    </div>
  );
}
