import { useEffect, useRef, useState } from "react";
import { api } from "../api";
import { errorMessage } from "../format";

type Turn = { role: "you" | "claude" | "note"; text: string };

// Answers take ~10s, so the panel has to make "still working" obvious — and
// offer a way out, because a slow model call must never pin the panel while
// the pick clock is running.
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
  // Bumped on every ask and on cancel. A result whose generation no longer
  // matches was cancelled while in flight and is discarded on arrival.
  const generation = useRef(0);

  useEffect(() => {
    if (open) inputRef.current?.focus();
  }, [open]);

  // Escape closes the panel from anywhere on the page. A handler on the panel
  // itself stops working the moment focus leaves it — clicking a suggestion
  // removes that button from the DOM and focus falls to <body>.
  useEffect(() => {
    if (!open) return;
    const onKey = (e: KeyboardEvent) => {
      if (e.key === "Escape") onClose();
    };
    document.addEventListener("keydown", onKey);
    return () => document.removeEventListener("keydown", onKey);
  }, [open, onClose]);

  // Keep the newest turn in view as answers land.
  useEffect(() => {
    const log = logRef.current;
    if (log) log.scrollTop = log.scrollHeight;
  }, [turns, busy]);

  const ask = async (text: string) => {
    const trimmed = text.trim();
    if (!trimmed || busy) return;
    const mine = ++generation.current;
    setError(null);
    setQuestion("");
    setTurns((prev) => [...prev, { role: "you", text: trimmed }]);
    setBusy(true);
    try {
      const answer = await api.chat(trimmed);
      if (mine !== generation.current) return;
      setTurns((prev) => [...prev, { role: "claude", text: answer }]);
    } catch (e) {
      if (mine !== generation.current) return;
      setError(errorMessage(e));
    } finally {
      if (mine === generation.current) {
        setBusy(false);
        // Hand focus back so Enter keeps working after a suggestion click.
        inputRef.current?.focus();
      }
    }
  };

  const cancel = () => {
    generation.current += 1;
    setBusy(false);
    setTurns((prev) => [...prev, { role: "note", text: "Cancelled — ask again whenever." }]);
    inputRef.current?.focus();
  };

  if (!open) return null;

  return (
    <aside className="chat" aria-label="Ask Claude about this draft">
      <header className="chat-head">
        <h3>Ask Claude</h3>
        <button className="ghost" onClick={onClose} aria-label="Close chat" title="Close (Esc)">
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
        {turns.map((turn, i) =>
          turn.role === "note" ? (
            <p key={i} className="chat-note muted">
              {turn.text}
            </p>
          ) : (
            <div key={i} className={`chat-turn ${turn.role}`}>
              <span className="chat-role">{turn.role === "you" ? "You" : "Claude"}</span>
              <p>{turn.text}</p>
            </div>
          ),
        )}
        {busy && (
          <div className="chat-turn claude pending" aria-live="polite">
            <span className="chat-role">Claude</span>
            <p className="muted">Thinking… (usually about 10 seconds)</p>
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
        {busy ? (
          <button className="ghost" onClick={cancel}>
            Cancel
          </button>
        ) : (
          <button onClick={() => void ask(question)} disabled={!question.trim()}>
            Ask
          </button>
        )}
      </div>
    </aside>
  );
}
