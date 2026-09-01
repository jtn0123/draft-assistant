// Saved conversations for the Ask Claude panel.
//
// One storage key per screen holds every chat for that screen, so the draft's
// conversations and the season's never mix. Storage is the same place the
// app's other remembered choices live (see `persisted.ts`); the panel is
// frontend-only and has no draft id to file a conversation under, so nothing
// here needs the backend. Every read and write is guarded: a browser that
// refuses to store still runs the panel, it just forgets afterwards.

import type { ChatMessage, ThreadEntry } from "./chat-types";

/** A whole conversation as the panel shows it, plus what it cost. */
export interface SavedChat {
  id: string;
  /** The first question, clipped — what the session list shows. */
  title: string;
  /** Unix milliseconds. */
  startedAt: number;
  updatedAt: number;
  entries: ThreadEntry[];
  /** What the next question will carry as context. */
  history: ChatMessage[];
  questions: number;
  costUsd: number;
}

/** What the picker needs, without the turns. */
export type ChatSessionSummary = Omit<SavedChat, "entries" | "history">;

/** Older conversations are dropped rather than filling the browser's quota. */
const MAX_SESSIONS = 20;
const TITLE_CHARS = 52;

const keyFor = (screen: string) => `da.chat.sessions.${screen}`;

/** Unique enough within a screen, and readable in the stored JSON. */
export function newSessionId(): string {
  const rand = Math.floor(Math.random() * 0xffff)
    .toString(16)
    .padStart(4, "0");
  return `${Date.now()}-${rand}`;
}

/** The first question, on one line and clipped. */
export function sessionTitle(entries: ThreadEntry[]): string {
  const first = entries.find((e) => e.kind === "me")?.lines[0] ?? "";
  const oneLine = first.replace(/\s+/g, " ").trim();
  if (oneLine === "") return "New chat";
  return oneLine.length > TITLE_CHARS ? `${oneLine.slice(0, TITLE_CHARS - 1)}…` : oneLine;
}

/** "12:41 · Who should I take? · 3 questions · $0.67" */
export function describeSession(s: ChatSessionSummary): string {
  const at = new Date(s.startedAt).toLocaleTimeString([], { hour: "numeric", minute: "2-digit" });
  const asked = `${s.questions} question${s.questions === 1 ? "" : "s"}`;
  return [at, s.title, asked, `$${s.costUsd.toFixed(2)}`].join(" · ");
}

function isEntry(value: unknown): value is ThreadEntry {
  if (typeof value !== "object" || value === null) return false;
  const entry = value as Partial<ThreadEntry>;
  return (
    typeof entry.id === "number" &&
    typeof entry.kind === "string" &&
    Array.isArray(entry.lines) &&
    entry.lines.every((line) => typeof line === "string")
  );
}

function isMessage(value: unknown): value is ChatMessage {
  if (typeof value !== "object" || value === null) return false;
  const message = value as Partial<ChatMessage>;
  return typeof message.role === "string" && typeof message.content === "string";
}

/** Stored text is only trusted as far as it checks out: a chat that does not
 *  is dropped rather than crashing the panel that opens it. */
function isSavedChat(value: unknown): value is SavedChat {
  if (typeof value !== "object" || value === null) return false;
  const chat = value as Partial<SavedChat>;
  return (
    typeof chat.id === "string" &&
    typeof chat.title === "string" &&
    typeof chat.startedAt === "number" &&
    typeof chat.updatedAt === "number" &&
    typeof chat.questions === "number" &&
    typeof chat.costUsd === "number" &&
    Array.isArray(chat.entries) &&
    chat.entries.every(isEntry) &&
    Array.isArray(chat.history) &&
    chat.history.every(isMessage)
  );
}

function readAll(screen: string): SavedChat[] {
  try {
    const raw = localStorage.getItem(keyFor(screen));
    if (raw === null) return [];
    const parsed: unknown = JSON.parse(raw);
    return Array.isArray(parsed) ? parsed.filter(isSavedChat) : [];
  } catch {
    return [];
  }
}

function writeAll(screen: string, chats: SavedChat[]): void {
  const newest = [...chats].sort((a, b) => b.updatedAt - a.updatedAt).slice(0, MAX_SESSIONS);
  try {
    localStorage.setItem(keyFor(screen), JSON.stringify(newest));
  } catch {
    // Not remembered for next time; the panel still shows the conversation.
  }
}

/** Newest activity first, like the picker lists them. */
export function listSessions(screen: string): ChatSessionSummary[] {
  return readAll(screen)
    .sort((a, b) => b.updatedAt - a.updatedAt)
    .map(({ id, title, startedAt, updatedAt, questions, costUsd }) => ({
      id,
      title,
      startedAt,
      updatedAt,
      questions,
      costUsd,
    }));
}

export function loadSession(screen: string, id: string): SavedChat | null {
  return readAll(screen).find((chat) => chat.id === id) ?? null;
}

/** Write the conversation whole; it replaces whatever was stored under its id. */
export function saveSession(screen: string, chat: SavedChat): void {
  writeAll(screen, [chat, ...readAll(screen).filter((stored) => stored.id !== chat.id)]);
}

export function deleteSession(screen: string, id: string): void {
  writeAll(
    screen,
    readAll(screen).filter((chat) => chat.id !== id),
  );
}
