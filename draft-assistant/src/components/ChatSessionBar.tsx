// The saved conversations for this screen, and what the panel is allowed to
// spend. Both are about the thread as a whole rather than the next question,
// so they sit together on one line above it.

import { describeSession, type ChatSessionSummary } from "../chatSessions";
import { formatUsd } from "../chatCost";

export function ChatSessionBar({
  sessions,
  currentId,
  saved,
  spent,
  screenSpent,
  budget,
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
  disabled: boolean;
  onOpen: (id: string) => void;
  onDelete: (id: string) => void;
  onBudget: (next: number) => void;
}) {
  const listed = sessions.some((s) => s.id === currentId);
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
        value={budget}
        aria-label="Spend cap in dollars"
        title="Asking stops once this screen's chats have cost this much, all conversations together. 0 means no cap."
        onChange={(e) => onBudget(Number(e.target.value))}
      />
      <span className="muted small chat-spend">
        {formatUsd(spent)} spent
        {screenSpent > spent && ` · ${formatUsd(screenSpent)} on this screen`}
      </span>
    </div>
  );
}
