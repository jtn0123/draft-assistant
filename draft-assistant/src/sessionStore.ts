// Browser-preview stand-in for the desktop session files: the same three
// operations over localStorage, one key per draft holding every session.
// Lets the panel's save / reopen / new-chat behaviour run (and be tested)
// in a plain browser tab, where there is no data dir to write to.

import type { ChatSession, ChatSessionSummary } from "./types";

const keyFor = (draftId: string) => `draft-assistant.chat-sessions.${draftId}`;

function readAll(draftId: string): Record<string, ChatSession> {
  try {
    const raw = window.localStorage.getItem(keyFor(draftId));
    return raw ? (JSON.parse(raw) as Record<string, ChatSession>) : {};
  } catch {
    return {};
  }
}

export function browserSaveSession(session: ChatSession): string {
  const all = readAll(session.draft_id);
  all[session.id] = session;
  window.localStorage.setItem(keyFor(session.draft_id), JSON.stringify(all));
  return `localStorage:${keyFor(session.draft_id)}/${session.id}`;
}

/** Newest activity first, like the desktop list. */
export function browserListSessions(draftId: string): ChatSessionSummary[] {
  return Object.values(readAll(draftId))
    .map((s) => ({
      id: s.id,
      title: s.title,
      started_at: s.started_at,
      updated_at: s.updated_at,
      questions: s.questions,
      cost_usd: s.cost_usd,
    }))
    .sort((a, b) => b.updated_at - a.updated_at || b.started_at - a.started_at);
}

export function browserLoadSession(draftId: string, id: string): ChatSession {
  const session = readAll(draftId)[id];
  if (!session) throw new Error(`chat session ${id} could not be read`);
  return session;
}
