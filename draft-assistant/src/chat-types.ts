// Mirrors the Rust chat structs.

export interface ChatMessage {
  /** "user" or "assistant" */
  role: string;
  content: string;
}

/** One chat turn on its way to the backend. A type alias, not an interface,
 * so it still satisfies Tauri's `Record<string, unknown>` invoke args. */
export type ChatRequest = {
  screen: string;
  model: string;
  effort: string;
  messages: ChatMessage[];
};

export interface ChatReply {
  text: string;
  thinking: string | null;
  model: string;
  refused: boolean;
  input_tokens: number;
  output_tokens: number;
  /** Which route answered this turn. */
  provider: "api" | "claude_code";
  /** What the turn cost: list price over the API, and $0 over the CLI, which
   * a subscription has already paid for. */
  cost_usd: number;
  /** What this screen's chats have cost in total, after this turn. */
  screen_spend_usd: number;
}

export interface ChatSettings {
  has_key: boolean;
  key_hint: string | null;
  /** Whether the Claude Code CLI was found on this machine. */
  cli_available: boolean;
  /** "api" or "claude_code" — the route answers will take. */
  provider: "api" | "claude_code";
  /** Where the key is kept: the macOS Keychain, or a file in this app's own
   * data directory when the Keychain is not available. */
  key_store: "keychain" | "file";
  /** Dollars a screen's chats may spend before the backend refuses the next
   * turn. 0 means the cap is off. */
  budget_usd: number;
  /** screen -> what that screen's chats have cost so far, every conversation
   * together. This is what the cap is checked against. */
  spend_usd: Record<string, number>;
  models: string[];
  /** Effort levels each model accepts — Fable 5 has no "Off". */
  efforts: Record<string, string[]>;
  /** label -> [tooltip, footer note] */
  notes: Record<string, [string, string]>;
}

/** A rendered turn in the thread, including local-only dividers and errors. */
export interface ThreadEntry {
  id: number;
  kind: "me" | "claude" | "divider" | "error";
  label?: string;
  lines: string[];
}
