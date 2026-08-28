import { useEffect, useRef, useState } from "react";
import { api } from "../api";
import { errorMessage } from "../format";
import type { ChatOptions, ChatUsage } from "../types";
import { ChatSettings, UsageLine } from "./ChatSettings";
import { loadOptions, saveOptions, toHistory, type Turn } from "./chatOptions";

// Answers take 15–40s with the whole board in context, so the panel has to
// make "still working" obvious — and offer a way out, because a slow model
// call must never pin the panel while the pick clock is running.
const SUGGESTIONS = [
  "Who should I take next?",
  "What position am I weakest at?",
  "Who is likely gone before my next pick?",
  "Which flagged players are a real injury risk?",
];

type Busy = "ask" | "compact" | null;

export function Chat({ open, onClose }: { open: boolean; onClose: () => void }) {
  const [turns, setTurns] = useState<Turn[]>([]);
  const [question, setQuestion] = useState("");
  const [busy, setBusy] = useState<Busy>(null);
  const [error, setError] = useState<string | null>(null);
  const [options, setOptions] = useState<ChatOptions>(loadOptions);
  const [usage, setUsage] = useState<ChatUsage | null>(null);
  const [session, setSession] = useState({ questions: 0, cost: 0 });
  const logRef = useRef<HTMLDivElement>(null);
  const inputRef = useRef<HTMLTextAreaElement>(null);
  // Bumped on every ask, compact, cancel, and new chat. A result whose
  // generation no longer matches was abandoned in flight and is discarded.
  const generation = useRef(0);
  // Say "fast mode did not serve" once per session, not under every answer.
  const fastNoted = useRef(false);

  useEffect(() => {
    if (open) inputRef.current?.focus();
  }, [open]);

  // Escape closes the panel from anywhere on the page. A handler on the panel
  // itself stops working the moment focus leaves it — clicking a suggestion
  // removes that button from the DOM and focus falls to <body>. While a
  // confirm dialog is open, Escape belongs to the dialog alone.
  useEffect(() => {
    if (!open) return;
    const onKey = (e: KeyboardEvent) => {
      if (e.key !== "Escape") return;
      if (document.querySelector("dialog[open]")) return;
      onClose();
    };
    document.addEventListener("keydown", onKey);
    return () => document.removeEventListener("keydown", onKey);
  }, [open, onClose]);

  // Keep the newest turn in view as answers land.
  useEffect(() => {
    const log = logRef.current;
    if (log) log.scrollTop = log.scrollHeight;
  }, [turns, busy]);

  const updateOptions = (next: ChatOptions) => {
    if (next.fast !== options.fast) fastNoted.current = false;
    setOptions(next);
    saveOptions(next);
  };

  const record = (reply: ChatUsage) => {
    setUsage(reply);
    setSession((s) => ({
      questions: s.questions + 1,
      cost: s.cost + (reply.cost_usd ?? 0),
    }));
  };

  const fastNote = (reply: ChatUsage): Turn[] => {
    if (!options.fast || reply.fast_mode === "active" || fastNoted.current) return [];
    fastNoted.current = true;
    const reason = reply.fast_mode_reason ? ` (${reply.fast_mode_reason})` : "";
    return [{ role: "note", text: `Fast mode unavailable${reason} — answered at standard speed.` }];
  };

  const finish = (mine: number) => {
    if (mine !== generation.current) return;
    setBusy(null);
    // Hand focus back so Enter keeps working after a suggestion click.
    inputRef.current?.focus();
  };

  const ask = async (text: string) => {
    const trimmed = text.trim();
    if (!trimmed || busy) return;
    const mine = ++generation.current;
    const history = toHistory(turns);
    setError(null);
    setQuestion("");
    setTurns((prev) => [...prev, { role: "you", text: trimmed }]);
    setBusy("ask");
    try {
      const reply = await api.chat(trimmed, history, options);
      if (mine !== generation.current) return;
      setTurns((prev) => [...prev, { role: "claude", text: reply.answer }, ...fastNote(reply.usage)]);
      record(reply.usage);
    } catch (e) {
      if (mine !== generation.current) return;
      setError(errorMessage(e));
    } finally {
      finish(mine);
    }
  };

  const compact = async () => {
    if (busy) return;
    const mine = ++generation.current;
    setError(null);
    setBusy("compact");
    try {
      const reply = await api.chatCompact(toHistory(turns), options);
      if (mine !== generation.current) return;
      setTurns([
        { role: "summary", text: reply.answer },
        { role: "note", text: "Earlier conversation folded into the summary above." },
      ]);
      setUsage(reply.usage);
      setSession((s) => ({ ...s, cost: s.cost + (reply.usage.cost_usd ?? 0) }));
    } catch (e) {
      if (mine !== generation.current) return;
      setError(errorMessage(e));
    } finally {
      finish(mine);
    }
  };

  const cancel = () => {
    generation.current += 1;
    setBusy(null);
    setTurns((prev) => [...prev, { role: "note", text: "Cancelled — ask again whenever." }]);
    inputRef.current?.focus();
  };

  const newChat = () => {
    generation.current += 1;
    fastNoted.current = false;
    setTurns([]);
    setUsage(null);
    setSession({ questions: 0, cost: 0 });
    setError(null);
    setBusy(null);
    inputRef.current?.focus();
  };

  if (!open) return null;

  const asked = turns.filter((t) => t.role === "you").length;
  const canCompact = busy === null && asked >= 2;

  return (
    <aside className="chat" aria-label="Ask Claude about this draft">
      <header className="chat-head">
        <h3>Ask Claude</h3>
        <div className="chat-head-actions">
          <button
            className="ghost small"
            onClick={newChat}
            disabled={turns.length === 0 && busy === null}
            title="Start over with an empty conversation"
          >
            New chat
          </button>
          <button
            className="ghost small"
            onClick={() => void compact()}
            disabled={!canCompact}
            title="Fold the conversation into a short summary so it stops costing its full length. Takes a minute or two."
          >
            Compact
          </button>
          <button className="ghost" onClick={onClose} aria-label="Close chat" title="Close (Esc)">
            ✕
          </button>
        </div>
      </header>

      <ChatSettings options={options} onChange={updateOptions} disabled={busy !== null} />

      <div className="chat-log" ref={logRef}>
        {turns.length === 0 && !busy && (
          <div className="chat-empty">
            <p className="muted">
              Claude sees the whole board with points, VORP, tiers, ADP and survival
              odds, every roster, recent picks, and the app&apos;s own recommendation
              — and remembers this conversation.
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
          ) : turn.role === "summary" ? (
            <div key={i} className="chat-turn summary">
              <span className="chat-role">Summary so far</span>
              <p>{turn.text}</p>
            </div>
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
            <p className="muted">
              {busy === "ask"
                ? "Thinking… (usually 15–40 seconds; longer answers take longer)"
                : "Compacting the conversation — this usually takes a minute or two…"}
            </p>
          </div>
        )}
        {error && (
          <div className="chat-error" role="alert">
            {error}
          </div>
        )}
      </div>

      <UsageLine usage={usage} questions={session.questions} cost={session.cost} />

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
