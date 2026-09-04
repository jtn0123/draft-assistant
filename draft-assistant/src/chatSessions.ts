// Saved conversations for the Ask Claude panel.
//
// One storage key per screen and league holds every chat filed under it, so
// the draft's conversations and the season's never mix — and neither do two
// leagues', since a question about one board means nothing about another.
// Storage is the same place the
// app's other remembered choices live (see `persisted.ts`); the panel is
// frontend-only and has no draft id to file a conversation under, so nothing
// here needs the backend. Every read and write is guarded: a browser that
// refuses to store still runs the panel, it just forgets afterwards.

import type { ChatMessage, ThreadEntry } from "./chat-types";
import { formatUsd } from "./chatCost";
import { dateLabel } from "./format";

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

/** The pinned conversation at the top of the picker: the one thread every
 *  paired phone and second screen shares. It is not stored here — the host
 *  owns it — so it is an id the panel recognises rather than a saved chat. */
export const SHARED_SESSION_ID = "shared";

/** What the picker calls it. */
export const SHARED_SESSION_LABEL = "Shared with devices";

/** Where one screen's conversations for one league are filed. Nothing
 *  migrates an older key: a league that has not been asked anything under this
 *  scheme simply opens on an empty thread, which is what a fresh league should
 *  do anyway. */
export const chatScope = (screen: string, leagueId: string): string => `${screen}.${leagueId}`;

const keyFor = (scope: string) => `da.chat.sessions.${scope}`;

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

/** "Sep 3, 12:41 PM · Who should I take? · 3 questions · $0.67"
 *
 * Eastern time and the shared dollar format, like every other timestamp and
 * amount in the app — a conversation filed at 12:41 stays filed at 12:41 when
 * the laptop crosses a time zone. */
export function describeSession(s: ChatSessionSummary): string {
  const asked = `${s.questions} question${s.questions === 1 ? "" : "s"}`;
  return [dateLabel(s.startedAt / 1000, true), s.title, asked, formatUsd(s.costUsd)].join(" · ");
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

function readAll(scope: string): SavedChat[] {
  try {
    const raw = localStorage.getItem(keyFor(scope));
    if (raw === null) return [];
    const parsed: unknown = JSON.parse(raw);
    return Array.isArray(parsed) ? parsed.filter(isSavedChat) : [];
  } catch {
    return [];
  }
}

function writeAll(scope: string, chats: SavedChat[]): void {
  const newest = [...chats].sort((a, b) => b.updatedAt - a.updatedAt).slice(0, MAX_SESSIONS);
  try {
    localStorage.setItem(keyFor(scope), JSON.stringify(newest));
  } catch {
    // Not remembered for next time; the panel still shows the conversation.
  }
}

/** Newest activity first, like the picker lists them. */
export function listSessions(scope: string): ChatSessionSummary[] {
  return readAll(scope)
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

export function loadSession(scope: string, id: string): SavedChat | null {
  return readAll(scope).find((chat) => chat.id === id) ?? null;
}

/** Write the conversation whole; it replaces whatever was stored under its id. */
export function saveSession(scope: string, chat: SavedChat): void {
  writeAll(scope, [chat, ...readAll(scope).filter((stored) => stored.id !== chat.id)]);
}

export function deleteSession(scope: string, id: string): void {
  writeAll(
    scope,
    readAll(scope).filter((chat) => chat.id !== id),
  );
}
