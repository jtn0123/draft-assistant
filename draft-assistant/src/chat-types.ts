// Mirrors the Rust chat structs.

export interface ChatMessage {
  /** "user" or "assistant" */
  role: string;
  content: string;
}

export interface ChatReply {
  text: string;
  thinking: string | null;
  model: string;
  refused: boolean;
  input_tokens: number;
  output_tokens: number;
}

export interface ChatSettings {
  has_key: boolean;
  key_hint: string | null;
  /** Whether the Claude Code CLI was found on this machine. */
  cli_available: boolean;
  /** "api" or "claude_code" — the route answers will take. */
  provider: "api" | "claude_code";
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
