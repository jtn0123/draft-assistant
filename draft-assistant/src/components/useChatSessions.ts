// The panel's saved conversations: which one this thread is, the others
// stored for this screen, and the writing of them. Split out of `Chat.tsx` so
// the panel keeps only what it renders.
//
// Nothing here runs in an effect. A conversation is written at the moment it
// changes — the panel calls `save` when a turn finishes — and the one to
// reopen is chosen while the panel's own state is being initialised, so the
// thread is never rendered empty and then replaced.

import { useCallback, useState } from "react";
import type { ChatMessage, ThreadEntry } from "../chat-types";
import {
  deleteSession,
  listSessions,
  loadSession,
  newSessionId,
  saveSession,
  sessionTitle,
  type ChatSessionSummary,
  type SavedChat,
} from "../chatSessions";

/** What one call to `save` records. */
export interface ChatSnapshot {
  entries: ThreadEntry[];
  history: ChatMessage[];
  questions: number;
  costUsd: number;
}

export interface ChatSessions {
  sessions: ChatSessionSummary[];
  sessionId: string;
  /** True once this conversation has been written. */
  saved: boolean;
  open: (id: string) => void;
  /** Begin a separate conversation; the stored one stays where it is. */
  startNew: () => void;
  /** Forget a stored conversation. Dropping the open one clears the panel. */
  remove: (id: string) => void;
  /** Write the conversation as it now stands. */
  save: (snapshot: ChatSnapshot) => void;
}

export interface Current {
  id: string;
  /** Unix milliseconds. */
  startedAt: number;
  saved: boolean;
}

/** What the panel opens with: the newest conversation stored under this
 *  scope, or a fresh one when the scope has none. Read once, while the panel's own
 *  state is being initialised, so a reopened thread is never rendered empty
 *  and then replaced. */
export interface ChatOpening {
  current: Current;
  reopened: SavedChat | null;
}

export function beginChat(scope: string): ChatOpening {
  const [newest] = listSessions(scope);
  const reopened = newest === undefined ? null : loadSession(scope, newest.id);
  if (reopened === null) {
    return { current: { id: newSessionId(), startedAt: Date.now(), saved: false }, reopened: null };
  }
  return {
    current: { id: reopened.id, startedAt: reopened.startedAt, saved: true },
    reopened,
  };
}

export function useChatSessions({
  scope,
  opening,
  onOpen,
  onClear,
}: {
  /** Which screen's chats, for which league — see `chatScope`. */
  scope: string;
  /** From `beginChat`, held by the panel so its thread starts on the same
   *  conversation this hook does. */
  opening: ChatOpening;
  onOpen: (chat: SavedChat) => void;
  onClear: () => void;
}): ChatSessions {
  const [current, setCurrent] = useState<Current>(opening.current);
  const [sessions, setSessions] = useState<ChatSessionSummary[]>(() => listSessions(scope));

  const open = useCallback(
    (id: string) => {
      const chat = loadSession(scope, id);
      if (chat === null) {
        setSessions(listSessions(scope));
        return;
      }
      setCurrent({ id: chat.id, startedAt: chat.startedAt, saved: true });
      onOpen(chat);
    },
    [scope, onOpen],
  );

  const startNew = useCallback(() => {
    setCurrent({ id: newSessionId(), startedAt: Date.now(), saved: false });
  }, []);

  const remove = useCallback(
    (id: string) => {
      deleteSession(scope, id);
      setSessions(listSessions(scope));
      if (id !== current.id) return;
      startNew();
      onClear();
    },
    [scope, current.id, startNew, onClear],
  );

  const save = useCallback(
    ({ entries, history, questions, costUsd }: ChatSnapshot) => {
      // A thread with nothing asked in it is not a conversation.
      if (!entries.some((entry) => entry.kind === "me")) return;
      saveSession(scope, {
        id: current.id,
        title: sessionTitle(entries),
        startedAt: current.startedAt,
        updatedAt: Date.now(),
        entries,
        history,
        questions,
        costUsd,
      });
      setCurrent((prev) => (prev.saved ? prev : { ...prev, saved: true }));
      setSessions(listSessions(scope));
    },
    [scope, current.id, current.startedAt],
  );

  return {
    sessions,
    sessionId: current.id,
    saved: current.saved,
    open,
    startNew,
    remove,
    save,
  };
}
