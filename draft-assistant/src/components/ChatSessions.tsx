import type { ChatSessionSummary } from "../types";
import { describeSession } from "./chatSession";

/**
 * The saved conversations for this draft. The current one is listed once it
 * has been saved; picking another reopens it in place (its context carries
 * on), and "New chat" in the header starts a fresh one.
 */
export function ChatSessions({
  sessions,
  currentId,
  savedTo,
  disabled,
  onSelect,
}: {
  sessions: ChatSessionSummary[];
  currentId: string;
  /** Where the current session was last written, or null before its first save. */
  savedTo: string | null;
  disabled: boolean;
  onSelect: (id: string) => void;
}) {
  const listed = sessions.some((s) => s.id === currentId);
  return (
    <div className="chat-sessions">
      <label>
        <span className="chat-sessions-label">Sessions</span>
        <select
          aria-label="Saved sessions"
          value={currentId}
          disabled={disabled}
          title={savedTo ? `Saved to ${savedTo}` : "Saved after the first answer"}
          onChange={(e) => {
            if (e.target.value !== currentId) onSelect(e.target.value);
          }}
        >
          {!listed && <option value={currentId}>Current chat (not saved yet)</option>}
          {sessions.map((s) => (
            <option key={s.id} value={s.id}>
              {describeSession(s)}
            </option>
          ))}
        </select>
      </label>
      <span className="muted small-text">
        {savedTo ? "saved" : sessions.length === 0 ? "nothing saved for this draft yet" : "not saved yet"}
      </span>
    </div>
  );
}
