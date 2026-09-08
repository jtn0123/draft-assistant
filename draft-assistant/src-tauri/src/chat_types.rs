//! The chat's own types: the models, the effort levels, a turn, a reply and
//! the error that carries what a failed one cost. Split out of `chat.rs`,
//! which keeps the call itself, for the line cap.

use serde::{Deserialize, Serialize};

/// The models the panel offers. Opus 5 can turn thinking off; Fable 5.1 cannot,
/// so its effort list starts at "low".
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub enum ChatModel {
    Opus5,
    Fable5,
    Astra,
    Sol,
}

impl ChatModel {
    pub fn id(self) -> &'static str {
        match self {
            ChatModel::Opus5 => "claude-opus-5",
            ChatModel::Fable5 => "claude-fable-5-1",
            ChatModel::Astra => "gpt-6-astra",
            ChatModel::Sol => "gpt-5.6-sol",
        }
    }

    pub fn parse(label: &str) -> Self {
        match label {
            // The two older spellings are what a saved config or a phone may still say.
            "Fable 5.1" | "claude-fable-5-1" | "Fable 5" | "claude-fable-5" => ChatModel::Fable5,
            "GPT-6 Astra" | "gpt-6-astra" => ChatModel::Astra,
            "GPT-5.6 Sol" | "gpt-5.6-sol" => ChatModel::Sol,
            _ => ChatModel::Opus5,
        }
    }

    /// Fable 5.1's thinking is always on — asking for it to be off is a 400.
    pub(crate) fn can_disable_thinking(self) -> bool {
        matches!(self, ChatModel::Opus5 | ChatModel::Sol)
    }

    pub fn is_openai(self) -> bool {
        matches!(self, Self::Astra | Self::Sol)
    }

    /// Published standard list price (verified 2026-09-07), in dollars per million tokens:
    /// (input, output). The one place prices live — the panel shows what the
    /// backend charged rather than pricing the turn a second time.
    pub fn price_per_mtok(self) -> (f64, f64) {
        match self {
            ChatModel::Opus5 => (5.0, 25.0),
            ChatModel::Fable5 => (10.0, 50.0),
            ChatModel::Astra => (10.0, 50.0),
            ChatModel::Sol => (4.0, 20.0),
        }
    }

    /// The model an answer *says* it was, mapped back to a price list.
    ///
    /// Not the same question as [`ChatModel::parse`], which reads a label off
    /// the panel's picker. What comes back is a dated id
    /// ("claude-opus-5-20260219"), and a server-side fallback can answer on a
    /// different model from the one asked for — so pricing the answer as the
    /// requested model charged the wrong rate, in either direction.
    pub fn from_reported(id: &str) -> Option<Self> {
        let id = id.to_ascii_lowercase();
        if id.starts_with("gpt-6-astra") {
            Some(Self::Astra)
        } else if id.starts_with("gpt-5.6-sol") || id == "gpt-5.6" {
            Some(Self::Sol)
        } else if id.contains("fable") {
            Some(ChatModel::Fable5)
        } else if id.contains("opus") {
            Some(ChatModel::Opus5)
        } else {
            None
        }
    }
}

/// What is added to an answer the model ran out of room for.
///
/// It names the effort level because thinking is billed against the same
/// ceiling the answer is: at a high effort most of the room can go on
/// reasoning nobody sees, and the same question at a lower one finishes.
pub const TRUNCATED_NOTE: &str =
    "Answer was cut off at the length limit. Ask for a shorter answer, or try a lower effort.";

/// Say so when the answer stops mid-thought. `stop_reason: "max_tokens"` used
/// to be read past in silence, so a truncated answer reached the panel looking
/// like a complete one that simply ended oddly.
pub(crate) fn with_truncation_note(text: String, truncated: bool) -> String {
    if !truncated {
        return text;
    }
    if text.trim().is_empty() {
        return TRUNCATED_NOTE.to_string();
    }
    format!("{text}\n\n{TRUNCATED_NOTE}")
}

/// How hard Claude should think. "Off" maps to disabled thinking, the rest to
/// the API's own effort levels.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Effort {
    Off,
    Low,
    Medium,
    High,
    XHigh,
    Max,
}

impl Effort {
    pub fn parse(label: &str) -> Self {
        match label.to_ascii_lowercase().as_str() {
            "off" => Effort::Off,
            "low" => Effort::Low,
            "medium" => Effort::Medium,
            "xhigh" => Effort::XHigh,
            "max" => Effort::Max,
            _ => Effort::High,
        }
    }

    /// The Claude Code CLI's `--effort` level. It has no "off"; low is the
    /// nearest thing.
    pub fn cli_effort(self) -> &'static str {
        match self {
            Effort::Off => "low",
            other => other.api_effort(),
        }
    }

    /// The `output_config.effort` value. Disabled thinking has no effort of
    /// its own; it rides at medium, which the API accepts alongside disabled.
    pub(crate) fn api_effort(self) -> &'static str {
        match self {
            Effort::Off => "medium",
            Effort::Low => "low",
            Effort::Medium => "medium",
            Effort::High => "high",
            Effort::XHigh => "xhigh",
            Effort::Max => "max",
        }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ChatMessage {
    /// "user" or "assistant".
    pub role: String,
    pub content: String,
}

#[derive(Debug, Clone, Serialize)]
pub struct ChatReply {
    pub text: String,
    /// Always `None`. Summarised reasoning is billed as output tokens and
    /// nothing renders it, so it is no longer asked for; the field stays
    /// because the panel's reply type has it.
    pub thinking: Option<String>,
    pub model: String,
    /// True when safety classifiers declined and no fallback rescued it.
    pub refused: bool,
    /// True when the answer hit the output limit and stops mid-thought. The
    /// note is already in `text`; this is for anything that wants to style it.
    pub truncated: bool,
    /// True when the user stopped the answer before it finished. `text` is
    /// whatever had arrived by then, which can be nothing at all.
    pub cancelled: bool,
    pub input_tokens: u32,
    pub output_tokens: u32,
    /// Prompt tokens written into the cache this turn, billed at 1.25x input.
    /// Zero on the CLI route, which reports no cache tiers.
    pub cache_creation_input_tokens: u32,
    /// Prompt tokens served from the cache, billed at 0.1x input.
    pub cache_read_input_tokens: u32,
    /// Which route answered: "api", "claude_code", or "codex". The transports do
    /// not know which one they are, so the command layer fills this in.
    pub provider: String,
    /// Estimated standard API-equivalent cost, including subscription routes.
    /// Not a bill or a spending limit. Filled in by the command layer.
    pub cost_usd: f64,
    /// Running estimated cost for this screen and league. No cap is enforced.
    pub screen_spend_usd: f64,
}

/// Why an answer did not arrive, and what it cost anyway.
///
/// A request the API accepted is billed from `message_start` on, whether or
/// not the rest of the stream reaches this side: a timeout or a dropped
/// socket halfway through an answer is still a charge. `partial` is the
/// usage that had arrived by then, so the command layer includes it in the
/// estimated cost instead of losing it with the error.
#[derive(Debug)]
pub struct ChatError {
    pub message: String,
    /// A reply with no text, carrying the model and the usage seen so far.
    /// `None` when nothing was billed: the request never reached the API, or
    /// was refused before it began. Boxed so the error stays small on the
    /// path that almost always carries only a message.
    pub partial: Option<Box<ChatReply>>,
}

impl From<String> for ChatError {
    fn from(message: String) -> Self {
        ChatError {
            message,
            partial: None,
        }
    }
}

impl From<ChatError> for String {
    fn from(error: ChatError) -> Self {
        error.message
    }
}
