// The panel's thread: the turns on screen, the history the next question
// carries, what the conversation has cost, and the one question that may be
// out. Split out of `Chat.tsx` so the panel keeps only what it renders.
//
// A question in flight is held by `chatPending`, not by this hook, so the
// panel can be closed or re-keyed while it is out: the next mount for the same
// scope picks the turn up still thinking, and a mount that never comes still
// has the answer filed into the conversation.

import { useCallback, useEffect, useRef, useState, type RefObject } from "react";
import { api } from "../api";
import type { ChatMessage, ThreadEntry } from "../chat-types";
import { cancelClaude } from "../chatCancel";
import {
  pendingTurn,
  startTurn,
  type PendingTurn,
  type Spend,
  type TurnOutcome,
} from "../chatPending";
import { listSessions, type SavedChat } from "../chatSessions";
import { beginChat, type ChatOpening, type ChatSnapshot } from "./useChatSessions";

/** What the panel opens on: the turn still out for this scope, in the
 *  conversation it was asked in, or else the newest saved conversation. */
export function resumeOrBegin(scope: string): {
  opening: ChatOpening;
  pending: PendingTurn | null;
} {
  const pending = pendingTurn(scope);
  if (pending === null) return { opening: beginChat(scope), pending: null };
  return {
    opening: {
      current: {
        id: pending.session.id,
        startedAt: pending.session.startedAt,
        saved: listSessions(scope).some((s) => s.id === pending.session.id),
      },
      reopened: null,
    },
    pending,
  };
}

/** The model and effort a question goes out with. */
export interface Asking {
  screen: string;
  model: string;
  effort: string;
}

export interface ChatThread {
  entries: ThreadEntry[];
  history: ChatMessage[];
  spend: Spend;
  /** True while a question is out, including one picked up from an earlier
   *  mount. */
  sending: boolean;
  /** What every conversation on this screen has cost together, as the
   *  backend last reported it. */
  screenSpend: number;
  setScreenSpend: (next: number) => void;
  /** Append a local-only turn: a divider, or an error from a control. */
  add: (entry: Omit<ThreadEntry, "id">) => void;
  /** Show a saved conversation. */
  show: (chat: SavedChat) => void;
  clear: () => void;
  /** Ask, and file the answer when it comes, whether or not this panel is
   *  still the one showing the thread. */
  send: (question: string, asking: Asking, session: { id: string; startedAt: number }) => void;
  /** Stop the answer that is out. It comes back marked cut short. */
  cancel: (screen: string) => void;
}

function nextIdAfter(entries: ThreadEntry[]): number {
  return Math.max(1, ...entries.map((e) => e.id + 1));
}

/**
 * @param file How a finished turn is filed into the saved chats. A ref, and
 *   one the panel fills in from an effect, because the saved-chats hook needs
 *   this hook's `show` and `clear` before it can exist.
 */
export function useChatThread(
  scope: string,
  opening: ChatOpening,
  pending: PendingTurn | null,
  file: RefObject<(snapshot: ChatSnapshot) => void>,
): ChatThread {
  const reopened = opening.reopened;
  const [entries, setEntries] = useState<ThreadEntry[]>(
    () => pending?.asked ?? reopened?.entries ?? [],
  );
  const [history, setHistory] = useState<ChatMessage[]>(
    () => pending?.outgoing ?? reopened?.history ?? [],
  );
  const [spend, setSpend] = useState<Spend>(
    () =>
      pending?.spend ?? { questions: reopened?.questions ?? 0, costUsd: reopened?.costUsd ?? 0 },
  );
  const [sending, setSending] = useState(pending !== null);
  const [screenSpend, setScreenSpend] = useState(0);
  const nextId = useRef(pending?.nextId ?? nextIdAfter(reopened?.entries ?? []));
  // False once this panel is gone, so a turn that finishes afterwards does not
  // set state on it. The turn itself is filed by `chatPending` either way.
  const live = useRef(true);
  useEffect(() => {
    live.current = true;
    return () => {
      live.current = false;
    };
  }, []);

  // Refs and setters only, so one `finish` serves every render.
  const finish = useCallback(
    (outcome: TurnOutcome) => {
      if (!live.current) return;
      nextId.current = outcome.nextId;
      setEntries(outcome.entries);
      setHistory(outcome.history);
      setSpend(outcome.spend);
      if (outcome.screenSpend !== null) setScreenSpend(outcome.screenSpend);
      setSending(false);
      file.current?.({ entries: outcome.entries, history: outcome.history, ...outcome.spend });
    },
    [file],
  );

  // A turn picked up from an earlier mount: the state above already shows
  // the question thinking; this is what lands the answer.
  useEffect(() => {
    if (pending === null) return;
    void pending.done.then(finish);
  }, [pending, finish]);

  const add = (entry: Omit<ThreadEntry, "id">) => {
    const id = nextId.current++;
    setEntries((prev) => [...prev, { ...entry, id }]);
  };

  const show = (chat: SavedChat) => {
    // Ids come back with the conversation, so a new turn cannot collide
    // with one that was stored.
    nextId.current = nextIdAfter(chat.entries);
    setEntries(chat.entries);
    setHistory(chat.history);
    setSpend({ questions: chat.questions, costUsd: chat.costUsd });
  };

  const clear = () => {
    setEntries([]);
    setHistory([]);
    setSpend({ questions: 0, costUsd: 0 });
  };

  const send = (question: string, asking: Asking, session: { id: string; startedAt: number }) => {
    // The turns are built here rather than only in state, so the conversation
    // can be filed the moment it stops moving without waiting for a render.
    const asked = [...entries, { id: nextId.current++, kind: "me" as const, lines: [question] }];
    const outgoing: ChatMessage[] = [...history, { role: "user", content: question }];
    setEntries(asked);
    setHistory(outgoing);
    setSending(true);
    const turn = startTurn(
      { scope, session, asked, before: history, outgoing, spend, nextId: nextId.current },
      api.askClaude({ ...asking, messages: outgoing }),
    );
    void turn.done.then(finish);
  };

  const cancel = (screen: string) => {
    void cancelClaude(screen);
  };

  return {
    entries,
    history,
    spend,
    sending,
    screenSpend,
    setScreenSpend,
    add,
    show,
    clear,
    send,
    cancel,
  };
}
