// What a conversation has cost, and the cap the user set on it.
//
// Nothing here prices a turn: the backend charges each answer against the
// published rates (`chat.rs`) and reports the dollars back, because it is also
// the side that enforces the cap and the side that knows the Claude Code route
// costs nothing per token. This file owns the display and the cap's local half.

import { persisted, usePersisted } from "./persisted";

/** "$0.42", or "$0.004" while a conversation is still worth less than a cent. */
export function formatUsd(amount: number): string {
  if (amount > 0 && amount < 0.01) return `$${amount.toFixed(3)}`;
  return `$${amount.toFixed(2)}`;
}

/** The cap in force, as the number of dollars. 0 means no cap. */
const budget = persisted<string>(
  "da.chatBudget",
  (raw) => (/^\d+(\.\d+)?$/.test(raw) ? raw : null),
  "5",
);

export function chatBudget(): number {
  return Number(budget.get());
}

/** Record a cap. A negative or non-finite one is not a cap and is ignored —
 *  whatever was in force stays in force. It used to be stored as "0", which is
 *  the one value that means *no cap*, so typing a minus sign removed the
 *  budget instead of being rejected. Returns whether it took. */
export function setChatBudget(next: number): boolean {
  if (!Number.isFinite(next) || next < 0) return false;
  budget.set(String(next));
  return true;
}

/** Read the cap in a component, re-rendering when it changes. */
export function useChatBudget(): number {
  return Number(usePersisted(budget));
}

/** Test seam: forget this session's cap and re-read what is stored. */
export function resetChatBudget(): void {
  budget.reset();
}

/** True once a conversation has spent everything the user allowed it. A
 *  warning only — the backend is what actually refuses the turn, against every
 *  conversation on the screen rather than only this one. */
export function overBudget(spent: number, cap: number): boolean {
  return cap > 0 && spent >= cap;
}
