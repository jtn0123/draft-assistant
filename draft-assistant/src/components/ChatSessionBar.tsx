// The saved conversations for this screen, and what the panel is allowed to
// spend. Both are about the thread as a whole rather than the next question,
// so they sit together on one line above it.

import { useState } from "react";
import { describeSession, type ChatSessionSummary } from "../chatSessions";
import { formatUsd } from "../chatCost";

/** What the box holds, read as a cap. `null` when it is not one. */
function readBudget(typed: string): number | null {
  const trimmed = typed.trim();
  if (trimmed === "") return null;
  const value = Number(trimmed);
  if (!Number.isFinite(value) || value < 0) return null;
  return value;
}

export function ChatSessionBar({
  sessions,
  currentId,
  saved,
  spent,
  screenSpent,
  budget,
  provider,
  disabled,
  onOpen,
  onDelete,
  onBudget,
}: {
  sessions: ChatSessionSummary[];
  currentId: string;
  /** True once this conversation has been written; false for a fresh one. */
  saved: boolean;
  spent: number;
  /** What every conversation on this screen has cost together. The cap is
   *  checked against this, not against `spent`. */
  screenSpent: number;
  /** Dollars this screen may spend before the backend refuses. 0 = no cap. */
  budget: number;
  /** Which route answers. The Claude Code one costs nothing per token, so the
   *  cap and the spend figure have nothing to count. */
  provider: "api" | "claude_code" | null;
  disabled: boolean;
  onOpen: (id: string) => void;
  onDelete: (id: string) => void;
  onBudget: (next: number) => void;
}) {
  const listed = sessions.some((s) => s.id === currentId);
  // What is in the box while it is being edited, or null when it is just
  // showing the cap. Emptying a number field is halfway through typing one,
  // and a controlled input that answered that with 0 would have quietly turned
  // the cap off — 0 is the one value that means "spend whatever you like".
  const [typed, setTyped] = useState<string | null>(null);
  const [rejected, setRejected] = useState<string | null>(null);
  const onSubscription = provider === "claude_code";

  /** Take what is in the box, or put it back. Nothing is stored while the
   *  user is still typing: every keystroke used to be committed, so "12"
   *  passed through a cap of 1 on its way, and a lone "-" through a cap of
   *  nothing at all. */
  const commit = () => {
    if (typed === null) return;
    const next = readBudget(typed);
    if (next === null) {
      // Keep the cap that was in force and say why this one was not taken.
      setRejected(
        typed.trim() === ""
          ? "A budget is a number of dollars. 0 turns the cap off."
          : `“${typed.trim()}” is not a budget — 0 turns the cap off.`,
      );
      setTyped(null);
      return;
    }
    setRejected(null);
    setTyped(null);
    if (next !== budget) onBudget(next);
  };

  return (
    <div className="chat-sessions">
      <span className="label">Chats</span>
      <select
        className="chat-session-pick"
        aria-label="Saved chats"
        value={currentId}
        disabled={disabled}
        onChange={(e) => {
          if (e.target.value !== currentId) onOpen(e.target.value);
        }}
      >
        {!listed && <option value={currentId}>This chat — nothing asked yet</option>}
        {sessions.map((s) => (
          <option key={s.id} value={s.id}>
            {describeSession(s)}
          </option>
        ))}
      </select>
      <button
        type="button"
        className="link-btn"
        onClick={() => onDelete(currentId)}
        disabled={disabled || !saved}
        title="Forget this saved conversation"
      >
        Delete
      </button>
      <span className="label chat-budget-label">Budget $</span>
      <input
        className="text-input chat-budget"
        type="number"
        min={0}
        step={1}
        value={typed ?? String(budget)}
        aria-label="Spend cap in dollars"
        aria-invalid={rejected !== null}
        title="Asking stops once this screen's chats have cost this much, all conversations together. 0 means no cap. Press Enter or click away to set it."
        onChange={(e) => {
          setTyped(e.target.value);
          setRejected(null);
        }}
        onKeyDown={(e) => {
          if (e.key === "Enter") commit();
          if (e.key === "Escape") {
            setTyped(null);
            setRejected(null);
          }
        }}
        onBlur={commit}
      />
      <span className="muted small chat-spend">
        {formatUsd(spent)} spent
        {screenSpent > spent && ` · ${formatUsd(screenSpent)} on this screen`}
      </span>
      {rejected !== null && (
        <span className="error small" role="alert">
          {rejected}
        </span>
      )}
      {onSubscription && (
        <span className="muted small">
          Answers from Claude Code are billed to your Claude subscription, not to this cap — which
          is why nothing is being counted here.
        </span>
      )}
    </div>
  );
}
