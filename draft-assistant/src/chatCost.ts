// What a conversation has cost, and the cap the user set on it.
//
// The backend reports the tokens each answer used; the prices below turn
// those into dollars so the panel can show a running total and stop asking
// once the user's cap is reached. Prices are Anthropic's list rates per
// million tokens for the two models the panel offers.

import { persisted, usePersisted } from "./persisted";

interface Price {
  /** Dollars per million input tokens. */
  input: number;
  /** Dollars per million output tokens. */
  output: number;
}

const PRICES: Record<string, Price> = {
  "Opus 5": { input: 5, output: 25 },
  "Fable 5": { input: 10, output: 50 },
};

/** The dearer of the two, so an unknown model is never under-counted. */
const FALLBACK: Price = { input: 10, output: 50 };

/** What one answer cost, in dollars. */
export function turnCost(model: string, inputTokens: number, outputTokens: number): number {
  const price = PRICES[model] ?? FALLBACK;
  return (inputTokens * price.input + outputTokens * price.output) / 1_000_000;
}

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

export function setChatBudget(next: number): void {
  budget.set(Number.isFinite(next) && next >= 0 ? String(next) : "0");
}

/** Read the cap in a component, re-rendering when it changes. */
export function useChatBudget(): number {
  return Number(usePersisted(budget));
}

/** Test seam: forget this session's cap and re-read what is stored. */
export function resetChatBudget(): void {
  budget.reset();
}

/** True once a conversation has spent everything the user allowed it. */
export function overBudget(spent: number, cap: number): boolean {
  return cap > 0 && spent >= cap;
}
