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
  /** The answer hit the length limit; the note is already in `text`. */
  truncated: boolean;
  /** The answer was stopped with Cancel. `text` is what had arrived, which
   * can be nothing. */
  cancelled: boolean;
  input_tokens: number;
  output_tokens: number;
  /** Which route answered this turn. */
  provider: "api" | "claude_code" | "codex";
  /** Estimated API-equivalent cost, including subscription routes.
   * Subscription estimates are not additional token bills. */
  cost_usd: number;
  /** What this screen's chats have cost in total, after this turn. */
  screen_spend_usd: number;
}

/** An answer while it is still arriving: the whole of the text so far.
 *
 *  The backend hands over what it has rather than the piece that just landed,
 *  so the panel replaces what it is showing and a missed event costs nothing.
 */
export interface ChatProgress {
  screen: string;
  text: string;
}

export interface ChatSettings {
  has_key: boolean;
  key_hint: string | null;
  /** Whether the Claude Code CLI was found on this machine. */
  cli_available: boolean;
  /** Whether ChatGPT-authenticated Codex is installed on this Mac. */
  codex_available?: boolean;
  /** "api" or "claude_code" — the route answers will take. */
  provider: "api" | "claude_code";
  /** Where the key is kept: the macOS Keychain, or a file in this app's own
   * data directory when the Keychain is not available. */
  key_store: "keychain" | "file";
  /** Legacy compatibility field; no spending cap is enforced. */
  budget_usd: number;
  /** Scope -> cumulative estimated API-equivalent cost. */
  spend_usd: Record<string, number>;
  models: string[];
  /** Effort levels each model accepts — Fable 5.1 has no "Off". */
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
