import type { ChatOptions, ChatTurn } from "../types";

/**
 * A line in the panel. `note` is panel-only (cancellations, fast-mode hints).
 * `asOfPick` is the pick an answer was written against.
 */
export type Turn = {
  role: "you" | "claude" | "summary" | "note";
  text: string;
  asOfPick?: number;
};

/** Sent by itself when the user's pick comes up and auto-ask is on. */
export const AUTO_QUESTION = "Who should I take next?";

/** Panel behaviour the backend never sees. */
export interface ChatPrefs {
  /** Ask "Who should I take next?" by itself when the user's pick comes up. */
  auto_ask: boolean;
  /** Stop asking once the session has cost this much. 0 = no limit. */
  budget_usd: number;
}

export const DEFAULT_PREFS: ChatPrefs = { auto_ask: false, budget_usd: 5 };

const PREFS_KEY = "draft-assistant.chat-prefs";

export function loadPrefs(): ChatPrefs {
  try {
    const raw = window.localStorage.getItem(PREFS_KEY);
    if (!raw) return DEFAULT_PREFS;
    const parsed = JSON.parse(raw) as Partial<ChatPrefs>;
    const budget = Number(parsed.budget_usd);
    return {
      auto_ask: parsed.auto_ask === true,
      budget_usd: Number.isFinite(budget) && budget >= 0 ? budget : DEFAULT_PREFS.budget_usd,
    };
  } catch {
    return DEFAULT_PREFS;
  }
}

export function savePrefs(prefs: ChatPrefs): void {
  try {
    window.localStorage.setItem(PREFS_KEY, JSON.stringify(prefs));
  } catch {
    // Storage can be unavailable (private mode); the choice still applies.
  }
}

export const MODELS = [
  { id: "opus", label: "Opus", hint: "Best judgement. Usually 15–40 s." },
  { id: "sonnet", label: "Sonnet", hint: "Quicker and cheaper; fine for lookups." },
  { id: "fable", label: "Fable", hint: "Newest and strongest; slowest." },
] as const;

export const EFFORTS = [
  { id: "", label: "Default" },
  { id: "low", label: "Low" },
  { id: "medium", label: "Medium" },
  { id: "high", label: "High" },
  { id: "xhigh", label: "Extra high" },
  { id: "max", label: "Max" },
] as const;

export const DEFAULT_OPTIONS: ChatOptions = {
  model: "opus",
  effort: null,
  fast: false,
  web_search: false,
};

const STORAGE_KEY = "draft-assistant.chat-options";

/** Last-used choices survive a restart; anything unrecognised falls back. */
export function loadOptions(): ChatOptions {
  try {
    const raw = window.localStorage.getItem(STORAGE_KEY);
    if (!raw) return DEFAULT_OPTIONS;
    const parsed = JSON.parse(raw) as Partial<ChatOptions>;
    const model = MODELS.some((m) => m.id === parsed.model)
      ? (parsed.model as string)
      : DEFAULT_OPTIONS.model;
    const effort =
      EFFORTS.some((e) => e.id === parsed.effort) && parsed.effort ? parsed.effort : null;
    return {
      model,
      effort,
      fast: parsed.fast === true,
      web_search: parsed.web_search === true,
    };
  } catch {
    return DEFAULT_OPTIONS;
  }
}

export function saveOptions(options: ChatOptions): void {
  try {
    window.localStorage.setItem(STORAGE_KEY, JSON.stringify(options));
  } catch {
    // Storage can be unavailable (private mode); the choice still applies.
  }
}

/** What the backend sees: the conversation, minus panel-only notes. */
export function toHistory(turns: Turn[]): ChatTurn[] {
  return turns.flatMap((t) => (t.role === "note" ? [] : [{ role: t.role, text: t.text }]));
}

export function formatTokens(n: number): string {
  if (n >= 1000) return `${(n / 1000).toFixed(1)}k`;
  return String(n);
}

export function formatSeconds(ms: number): string {
  return `${Math.max(1, Math.round(ms / 1000))} s`;
}

/**
 * One-line summary for the folded settings header, e.g.
 * "Opus · high effort · standard speed · web on · auto-ask · $5 budget".
 *
 * `prefs` is included because the two settings that change what the panel
 * does on its own — whether it asks unprompted, and what it will spend
 * before it stops — were invisible with the fold closed.
 */
export function describeOptions(o: ChatOptions, prefs?: ChatPrefs): string {
  const model = MODELS.find((m) => m.id === o.model)?.label ?? o.model;
  const effort = EFFORTS.find((e) => e.id === (o.effort ?? ""))?.label ?? "Default";
  const parts = [
    model,
    `${effort.toLowerCase()} effort`,
    o.fast ? "fast" : "standard speed",
    o.web_search ? "web on" : "web off",
  ];
  if (prefs?.auto_ask) parts.push("auto-ask");
  if (prefs) parts.push(prefs.budget_usd > 0 ? `$${prefs.budget_usd} budget` : "no budget");
  return parts.join(" · ");
}
