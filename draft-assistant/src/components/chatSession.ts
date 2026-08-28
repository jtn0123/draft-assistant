import type { ChatSession, ChatSessionSummary } from "../types";
import type { Turn } from "./chatOptions";

/** Safe as a file name on the desktop side, unique enough within a draft. */
export function newSessionId(): string {
  const rand = Math.floor(Math.random() * 0xffff)
    .toString(16)
    .padStart(4, "0");
  return `${Date.now()}-${rand}`;
}

export const nowSecs = () => Math.floor(Date.now() / 1000);

const TITLE_CHARS = 60;

/** The first question, clipped — what the session list shows. */
export function sessionTitle(turns: Turn[]): string {
  const first = turns.find((t) => t.role === "you")?.text.trim() ?? "";
  const oneLine = first.replace(/\s+/g, " ");
  return oneLine.length > TITLE_CHARS ? `${oneLine.slice(0, TITLE_CHARS - 1)}…` : oneLine;
}

export function toSession(args: {
  id: string;
  draftId: string;
  leagueName: string;
  startedAt: number;
  turns: Turn[];
  questions: number;
  cost: number;
}): ChatSession {
  return {
    id: args.id,
    draft_id: args.draftId,
    league_name: args.leagueName,
    started_at: args.startedAt,
    updated_at: nowSecs(),
    title: sessionTitle(args.turns),
    turns: args.turns.map((t) => ({ role: t.role, text: t.text, as_of_pick: t.asOfPick ?? null })),
    questions: args.questions,
    cost_usd: args.cost,
  };
}

export function fromSession(session: ChatSession): Turn[] {
  return session.turns.map((t) => ({
    role: t.role,
    text: t.text,
    ...(t.as_of_pick !== null ? { asOfPick: t.as_of_pick } : {}),
  }));
}

/** "12:41 · Who should I take? · 3 questions · $0.67" */
export function describeSession(s: ChatSessionSummary): string {
  const at = new Date(s.started_at * 1000).toLocaleTimeString([], {
    hour: "numeric",
    minute: "2-digit",
  });
  const n = `${s.questions} question${s.questions === 1 ? "" : "s"}`;
  return [at, s.title || "(no question yet)", n, `$${s.cost_usd.toFixed(2)}`].join(" · ");
}
