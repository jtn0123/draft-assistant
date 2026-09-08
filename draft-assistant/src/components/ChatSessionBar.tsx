// Saved conversations and their estimated API-equivalent usage cost.

import {
  describeSession,
  SHARED_SESSION_ID,
  SHARED_SESSION_LABEL,
  type ChatSessionSummary,
} from "../chatSessions";
import { formatUsd } from "../chatCost";

export function ChatSessionBar({
  sessions,
  currentId,
  shared,
  onShared,
  saved,
  spent,
  screenSpent,
  provider,
  disabled,
  onOpen,
  onDelete,
}: {
  sessions: ChatSessionSummary[];
  currentId: string;
  /** True while the pinned shared thread is the one on screen. */
  shared: boolean;
  /** Switch to or away from the shared thread. */
  onShared: (next: boolean) => void;
  /** True once this conversation has been written; false for a fresh one. */
  saved: boolean;
  spent: number;
  /** Estimated cost of every conversation on this screen together. */
  screenSpent: number;
  provider: "api" | "claude_code" | "codex" | null;
  disabled: boolean;
  onOpen: (id: string) => void;
  onDelete: (id: string) => void;
}) {
  const listed = sessions.some((s) => s.id === currentId);
  const onSubscription = provider === "claude_code" || provider === "codex";
  return (
    <div className="chat-sessions">
      <span className="label">Chats</span>
      <select
        className="chat-session-pick"
        aria-label="Saved chats"
        value={shared ? SHARED_SESSION_ID : currentId}
        disabled={disabled}
        onChange={(e) => {
          const picked = e.target.value;
          if (picked === SHARED_SESSION_ID) {
            onShared(true);
            return;
          }
          onShared(false);
          if (picked !== currentId) onOpen(picked);
        }}
      >
        {/* Pinned above the saved ones, and always there: the thread the
            phones are reading is not one of this Mac's conversations. */}
        <option value={SHARED_SESSION_ID}>{SHARED_SESSION_LABEL}</option>
        {!listed && <option value={currentId}>This chat, nothing asked yet</option>}
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
        disabled={disabled || shared || !saved}
        title="Forget this saved conversation"
      >
        Delete
      </button>
      <span className="muted small chat-spend">
        {formatUsd(spent)} estimated cost
        {screenSpent > spent && ` · ${formatUsd(screenSpent)} on this screen`}
      </span>
      {onSubscription && (
        <span className="muted small">
          API-equivalent estimate for comparison. Subscription calls are not extra token bills.
        </span>
      )}
    </div>
  );
}
