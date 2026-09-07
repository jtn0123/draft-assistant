// The question that is out right now, kept outside the panel.
//
// The panel is unmounted when it is closed and re-keyed when the screen or
// the league changes, and a turn that was in flight went with it: the
// question vanished from the thread, the answer arrived to nothing, and what
// it cost was never added to the conversation. The turn now lives here, at
// module level, from the moment it is sent until it is filed. A panel that
// mounts while one is out picks it up and shows it still thinking; a panel
// that never comes back still gets the answer saved into the conversation.

import type { ChatMessage, ChatReply, ThreadEntry } from "./chat-types";
import { saveSession, sessionTitle } from "./chatSessions";
import { describeError } from "./errorText";

export interface Spend {
  questions: number;
  costUsd: number;
}

/** Everything a turn needs to be finished without the panel that sent it. */
export interface TurnStart {
  scope: string;
  /** The conversation the turn belongs to, so it can be filed. */
  session: { id: string; startedAt: number };
  /** The thread as shown, with the question appended. */
  asked: ThreadEntry[];
  /** The history before the question, for a turn that fails. */
  before: ChatMessage[];
  /** The history with the question: what went to the backend. */
  outgoing: ChatMessage[];
  /** What the conversation had cost before this turn. */
  spend: Spend;
  /** The next unused entry id. */
  nextId: number;
}

/** How a turn ended, as the panel should now stand. */
export interface TurnOutcome {
  entries: ThreadEntry[];
  history: ChatMessage[];
  spend: Spend;
  /** The backend's total for this screen after the turn; null on a failure,
   *  which reports none. */
  screenSpend: number | null;
  nextId: number;
}

export interface PendingTurn extends TurnStart {
  done: Promise<TurnOutcome>;
}

const pending = new Map<string, PendingTurn>();

/** The label a Claude turn carries, when it carries one. */
function labelFor(reply: ChatReply): string | undefined {
  if (reply.cancelled) return "Cut short";
  if (reply.refused) return "Declined";
  return undefined;
}

/** The outcome of a reply, including one that was stopped early. A cancel
 *  with text keeps the text, marked; one with none reads as a failed turn, so
 *  the question is not left in the history for the next turn to resend. */
function answered(start: TurnStart, reply: ChatReply): TurnOutcome {
  const spend = {
    questions: start.spend.questions + 1,
    costUsd: start.spend.costUsd + reply.cost_usd,
  };
  if (reply.cancelled && reply.text.trim() === "") {
    return {
      entries: [
        ...start.asked,
        { id: start.nextId, kind: "error", lines: ["Cancelled before an answer arrived."] },
      ],
      history: start.before,
      spend,
      screenSpend: reply.screen_spend_usd,
      nextId: start.nextId + 1,
    };
  }
  return {
    entries: [
      ...start.asked,
      {
        id: start.nextId,
        kind: "claude",
        label: labelFor(reply),
        lines: reply.text.split("\n\n").filter((l) => l.trim() !== ""),
      },
    ],
    history: [...start.outgoing, { role: "assistant", content: reply.text }],
    spend,
    screenSpend: reply.screen_spend_usd,
    nextId: start.nextId + 1,
  };
}

/** A failed turn: the error is a turn in the thread, and the question is
 *  dropped from the history so a retry does not resend it. */
function failed(start: TurnStart, error: unknown): TurnOutcome {
  return {
    entries: [...start.asked, { id: start.nextId, kind: "error", lines: [describeError(error)] }],
    history: start.before,
    spend: start.spend,
    screenSpend: null,
    nextId: start.nextId + 1,
  };
}

/** File the turn's outcome into its conversation. The panel does the same
 *  when it is mounted; this is for when it is not. */
function file(start: TurnStart, outcome: TurnOutcome): void {
  saveSession(start.scope, {
    id: start.session.id,
    title: sessionTitle(outcome.entries),
    startedAt: start.session.startedAt,
    updatedAt: Date.now(),
    entries: outcome.entries,
    history: outcome.history,
    questions: outcome.spend.questions,
    costUsd: outcome.spend.costUsd,
  });
}

/** Send a turn and remember it until it is filed. */
export function startTurn(start: TurnStart, request: Promise<ChatReply>): PendingTurn {
  const done = request
    .then(
      (reply) => answered(start, reply),
      (error: unknown) => failed(start, error),
    )
    .then((outcome) => {
      // Only this turn's own entry: a later one under the same scope stays.
      if (pending.get(start.scope) === turn) pending.delete(start.scope);
      file(start, outcome);
      return outcome;
    });
  const turn: PendingTurn = { ...start, done };
  pending.set(start.scope, turn);
  return turn;
}

/** The turn out for this scope, if one is. */
export function pendingTurn(scope: string): PendingTurn | null {
  return pending.get(scope) ?? null;
}

/** Forget every pending turn. Only the tests use this. */
export function resetPendingTurns(): void {
  pending.clear();
}
