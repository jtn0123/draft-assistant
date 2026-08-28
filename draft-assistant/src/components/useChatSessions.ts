import { useEffect, useRef, useState } from "react";
import { api } from "../api";
import { errorMessage } from "../format";
import type { ChatSession, ChatSessionSummary } from "../types";
import { newSessionId, nowSecs, toSession } from "./chatSession";
import type { Turn } from "./chatOptions";

/**
 * The panel's saved conversations: which file this thread is, the others on
 * disk for the draft, and when to write. Split out of `Chat.tsx` so the
 * panel keeps only what it renders.
 *
 * The panel owns the conversation itself and applies a reopened one through
 * `onRestore`; this hook owns only the bookkeeping around it.
 */
export function useChatSessions({
  draftId,
  leagueName,
  busy,
  turns,
  questions,
  cost,
  onRestore,
  onError,
}: {
  draftId?: string;
  leagueName: string;
  /** True while an answer or a compaction is in flight; nothing is written then. */
  busy: boolean;
  turns: Turn[];
  questions: number;
  cost: number;
  onRestore: (session: ChatSession) => void;
  onError: (message: string) => void;
}) {
  const [sessions, setSessions] = useState<ChatSessionSummary[]>([]);
  const [sessionId, setSessionId] = useState(newSessionId);
  const [savedTo, setSavedTo] = useState<string | null>(null);
  const [saved, setSaved] = useState(false);
  const startedAt = useRef(nowSecs());
  // True once something happened that is not on disk yet. Reopening a saved
  // session must not count, or every reopen would rewrite its file.
  const dirty = useRef(false);

  const refreshSessions = async (id: string) => {
    try {
      setSessions(await api.listChatSessions(id));
    } catch {
      // Listing failing means saving will too; the save reports it.
    }
  };

  const openSession = async (id: string) => {
    if (!draftId || busy) return;
    try {
      const stored = await api.loadChatSession(draftId, id);
      dirty.current = false;
      setSessionId(stored.id);
      startedAt.current = stored.started_at;
      setSavedTo(null);
      setSaved(true);
      onRestore(stored);
    } catch (e) {
      onError(errorMessage(e));
    }
  };

  /** Begin a separate conversation; the previous file stays where it is. */
  const startNew = () => {
    dirty.current = false;
    setSessionId(newSessionId());
    startedAt.current = nowSecs();
    setSavedTo(null);
    setSaved(false);
  };

  const markDirty = () => {
    dirty.current = true;
  };

  // The latest `openSession` for the restore effect, which runs once per
  // draft and must not re-run as the function closes over fresh state.
  const openSessionRef = useRef(openSession);
  useEffect(() => {
    openSessionRef.current = openSession;
  });

  // When a draft is loaded, pick up where the last conversation left off —
  // a reload or a relaunch should not cost the evening's answers. Runs once
  // per draft; StrictMode's dev-only double mount cancels the first run and
  // the second completes, which is why there is no "already restored" flag.
  useEffect(() => {
    if (!draftId) return;
    let cancelled = false;
    void (async () => {
      let list: ChatSessionSummary[];
      try {
        list = await api.listChatSessions(draftId);
      } catch {
        return;
      }
      if (cancelled) return;
      setSessions(list);
      if (list.length > 0 && !dirty.current) void openSessionRef.current(list[0].id);
    })();
    return () => {
      cancelled = true;
    };
  }, [draftId]);

  // Save after every completed answer, compaction or cancel: the file on
  // disk is whatever the panel shows once it has stopped moving.
  useEffect(() => {
    if (!draftId || busy || !dirty.current) return;
    if (!turns.some((t) => t.role === "you" || t.role === "summary")) return;
    dirty.current = false;
    const snapshot = toSession({
      id: sessionId,
      draftId,
      leagueName,
      startedAt: startedAt.current,
      turns,
      questions,
      cost,
    });
    void api
      .saveChatSession(snapshot)
      .then((path) => {
        setSavedTo(path);
        setSaved(true);
        return refreshSessions(draftId);
      })
      .catch((e: unknown) => onError(`Could not save this chat: ${errorMessage(e)}`));
    // `onError` and `onRestore` are recreated every render by design.
    // eslint-disable-next-line react-hooks/exhaustive-deps
  }, [turns, busy, draftId, leagueName, sessionId, questions, cost]);

  return { sessions, sessionId, saved, savedTo, openSession, startNew, markDirty };
}
