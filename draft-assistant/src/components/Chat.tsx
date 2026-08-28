import { useEffect, useRef, useState } from "react";
import { api } from "../api";

type Turn = { role: "you" | "claude"; text: string };

// Answers take ~10s, so the panel has to make "still working" obvious.
const SUGGESTIONS = [
  "Who should I take next?",
  "What position am I weakest at?",
  "Who is likely gone before my next pick?",
];

export function Chat({ open, onClose }: { open: boolean; onClose: () => void }) {
  const [turns, setTurns] = useState<Turn[]>([]);
  const [question, setQuestion] = useState("");
  const [busy, setBusy] = useState(false);
  const [error, setError] = useState<string | null>(null);
  const logRef = useRef<HTMLDivElement>(null);
  const inputRef = useRef<HTMLTextAreaElement>(null);

  useEffect(() => {
    if (open) inputRef.current?.focus();
  }, [open]);

  // Keep the newest turn in view as answers land.
  useEffect(() => {
    const log = logRef.current;
    if (log) log.scrollTop = log.scrollHeight;
  }, [turns, busy]);

  const ask = async (text: string) => {
    const trimmed = text.trim();
    if (!trimmed || busy) return;
    setError(null);
    setQuestion("");
    setTurns((prev) => [...prev, { role: "you", text: trimmed }]);
    setBusy(true);
    try {
      const answer = await api.chat(trimmed);
      setTurns((prev) => [...prev, { role: "claude", text: answer }]);
    } catch (e) {
      setError(String(e));
    } finally {
      setBusy(false);
    }
  };

  if (!open) return null;

  return (
    <aside className="chat" aria-label="Ask Claude about this draft">
      <header className="chat-head">
        <h3>Ask Claude</h3>
        <button className="ghost" onClick={onClose} aria-label="Close chat">
          ✕
        </button>
      </header>

      <div className="chat-log" ref={logRef}>
        {turns.length === 0 && !busy && (
          <div className="chat-empty">
            <p className="muted">
              Claude sees your roster, the top of the board, survival odds, and
              tier alerts — the same state the app reasons from.
            </p>
            {SUGGESTIONS.map((s) => (
              <button key={s} className="chat-suggestion" onClick={() => void ask(s)}>
                {s}
              </button>
            ))}
          </div>
        )}
        {turns.map((turn, i) => (
          <div key={i} className={`chat-turn ${turn.role}`}>
            <span className="chat-role">{turn.role === "you" ? "You" : "Claude"}</span>
            <p>{turn.text}</p>
          </div>
        ))}
        {busy && (
          <div className="chat-turn claude pending" aria-live="polite">
            <span className="chat-role">Claude</span>
            <p className="muted">Thinking… (about 10 seconds)</p>
          </div>
        )}
        {error && (
          <div className="chat-error" role="alert">
            {error}
          </div>
        )}
      </div>

      <div className="chat-input">
        <textarea
          ref={inputRef}
          value={question}
          rows={2}
          placeholder="Ask about the draft…"
          aria-label="Your question"
          onChange={(e) => setQuestion(e.target.value)}
          onKeyDown={(e) => {
            // Enter sends; Shift+Enter is a newline.
            if (e.key === "Enter" && !e.shiftKey) {
              e.preventDefault();
              void ask(question);
            }
          }}
        />
        <button onClick={() => void ask(question)} disabled={busy || !question.trim()}>
          {busy ? "Asking…" : "Ask"}
        </button>
      </div>
    </aside>
  );
}
