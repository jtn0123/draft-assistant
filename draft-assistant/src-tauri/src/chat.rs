//! The "Ask Claude" panel's backend: the Anthropic Messages API over raw HTTP.
//!
//! Rust has no official Anthropic SDK, so this speaks the wire format
//! directly. The board, roster and clock are passed as a system prompt rather
//! than pasted into every user turn.
//!
//! The request body, cache breakpoint included, is in `chat_request.rs`; what
//! is here is the call and everything that happens to the reply.

use serde::{Deserialize, Serialize};

const ENDPOINT: &str = "https://api.anthropic.com/v1/messages";
const API_VERSION: &str = "2023-06-01";
/// Opt into server-side refusal fallbacks (the `fallbacks: "default"` form).
const FALLBACK_BETA: &str = "server-side-fallback-2026-07-01";

/// The request body's own types, its cache breakpoints and its ceilings.
#[path = "chat_request.rs"]
mod request;

use request::build_request;

/// The models the panel offers. Opus 5 can turn thinking off; Fable 5 cannot,
/// so its effort list starts at "low".
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub enum ChatModel {
    Opus5,
    Fable5,
}

impl ChatModel {
    pub fn id(self) -> &'static str {
        match self {
            ChatModel::Opus5 => "claude-opus-5",
            ChatModel::Fable5 => "claude-fable-5",
        }
    }

    pub fn parse(label: &str) -> Self {
        match label {
            "Fable 5" | "claude-fable-5" => ChatModel::Fable5,
            _ => ChatModel::Opus5,
        }
    }

    /// Fable 5's thinking is always on — asking for it to be off is a 400.
    pub(crate) fn can_disable_thinking(self) -> bool {
        matches!(self, ChatModel::Opus5)
    }

    /// Anthropic's published list price, in dollars per million tokens:
    /// (input, output). The one place prices live — the panel shows what the
    /// backend charged rather than pricing the turn a second time.
    pub fn price_per_mtok(self) -> (f64, f64) {
        match self {
            ChatModel::Opus5 => (5.0, 25.0),
            ChatModel::Fable5 => (10.0, 50.0),
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
        if id.contains("fable") {
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

/// Writing a token into the prompt cache costs a quarter more than sending it
/// plainly; reading one back costs a tenth. Anthropic's published multipliers.
const CACHE_WRITE_MULTIPLIER: f64 = 1.25;
const CACHE_READ_MULTIPLIER: f64 = 0.1;

/// What one answer cost at list price, in dollars, counting only the tokens
/// billed at the plain input and output rates.
pub fn turn_cost(model: ChatModel, input_tokens: u32, output_tokens: u32) -> f64 {
    let (input, output) = model.price_per_mtok();
    (f64::from(input_tokens) * input + f64::from(output_tokens) * output) / 1_000_000.0
}

/// What a whole reply cost, cache tiers included.
///
/// The system prompt is sent with a `cache_control: ephemeral` breakpoint, so
/// on a turn that hits the cache most of the prompt is billed as a cache read
/// and none of it appears in `input_tokens`. Pricing a turn from `input_tokens` alone
/// therefore undercounts it — badly on the turn that writes the cache, which
/// is charged at a premium — and the panel's running spend drifts under the
/// cap it is supposed to enforce.
pub fn turn_cost_of(model: ChatModel, reply: &ChatReply) -> f64 {
    let (input, _) = model.price_per_mtok();
    let cached = f64::from(reply.cache_creation_input_tokens) * input * CACHE_WRITE_MULTIPLIER
        + f64::from(reply.cache_read_input_tokens) * input * CACHE_READ_MULTIPLIER;
    turn_cost(model, reply.input_tokens, reply.output_tokens) + cached / 1_000_000.0
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
    fn api_effort(self) -> &'static str {
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
    pub input_tokens: u32,
    pub output_tokens: u32,
    /// Prompt tokens written into the cache this turn, billed at 1.25x input.
    /// Zero on the CLI route, which reports no cache tiers.
    pub cache_creation_input_tokens: u32,
    /// Prompt tokens served from the cache, billed at 0.1x input.
    pub cache_read_input_tokens: u32,
    /// Which route answered: "api" or "claude_code". The transports below do
    /// not know which one they are, so the command layer fills this in.
    pub provider: String,
    /// What this turn cost in dollars — list price on the API route, and zero
    /// on the CLI route, which is paid for by a subscription rather than by
    /// the token. Also filled in by the command layer.
    pub cost_usd: f64,
    /// What this screen's chat has cost in total, after this turn, against
    /// the cap in `chat_budget_usd`.
    pub screen_spend_usd: f64,
}

// ---------- response wire types ----------

#[derive(Deserialize)]
struct ContentBlock {
    #[serde(rename = "type")]
    kind: String,
    #[serde(default)]
    text: Option<String>,
}

#[derive(Deserialize, Default)]
struct Usage {
    #[serde(default)]
    input_tokens: u32,
    #[serde(default)]
    output_tokens: u32,
    /// Absent on a response that used no cache at all, hence the defaults.
    #[serde(default)]
    cache_creation_input_tokens: u32,
    #[serde(default)]
    cache_read_input_tokens: u32,
}

#[derive(Deserialize)]
struct Response {
    #[serde(default)]
    content: Vec<ContentBlock>,
    #[serde(default)]
    model: String,
    #[serde(default)]
    stop_reason: Option<String>,
    #[serde(default)]
    usage: Usage,
}

#[derive(Deserialize)]
struct ApiErrorBody {
    #[serde(default)]
    error: Option<ApiErrorDetail>,
}

#[derive(Deserialize)]
struct ApiErrorDetail {
    #[serde(default)]
    message: Option<String>,
}

/// Ask Claude about the current board.
///
/// `context` is the serialized view (draft or season) the panel is showing,
/// in the two halves the cache breakpoint goes between.
pub async fn ask(
    http: &reqwest::Client,
    api_key: &str,
    model: ChatModel,
    effort: Effort,
    context: &crate::chat_context::SplitContext,
    messages: &[ChatMessage],
) -> Result<ChatReply, String> {
    ask_at(ENDPOINT, http, api_key, model, effort, context, messages).await
}

/// The same request against an arbitrary endpoint. Only [`ask`] and the wire
/// tests, which point it at a stub server, call this.
async fn ask_at(
    endpoint: &str,
    http: &reqwest::Client,
    api_key: &str,
    model: ChatModel,
    effort: Effort,
    context: &crate::chat_context::SplitContext,
    messages: &[ChatMessage],
) -> Result<ChatReply, String> {
    if api_key.trim().is_empty() {
        return Err("no Anthropic API key set — add one in Settings".into());
    }
    if messages.is_empty() {
        return Err("nothing to ask".into());
    }

    let request = build_request(model, effort, context, messages);

    let response = http
        .post(endpoint)
        .header("x-api-key", api_key)
        .header("anthropic-version", API_VERSION)
        .header("anthropic-beta", FALLBACK_BETA)
        .header("content-type", "application/json")
        .json(&request)
        .send()
        .await
        .map_err(|e| format!("could not reach the Anthropic API: {e}"))?;

    let status = response.status();
    let body = response
        .text()
        .await
        .map_err(|e| format!("could not read the Anthropic response: {e}"))?;

    if !status.is_success() {
        let detail = serde_json::from_str::<ApiErrorBody>(&body)
            .ok()
            .and_then(|b| b.error.and_then(|e| e.message));
        if detail.is_none() {
            // Anything between here and Anthropic can answer with its own
            // error page. Pasting a gateway's HTML into the chat panel tells
            // the user nothing and looks like the model said it, so the body
            // goes to the log and the status speaks for itself.
            let logged: String = body.chars().take(300).collect();
            crate::applog::warn(format!(
                "Anthropic API {status} with a body that is not an error object: {logged}"
            ));
        }
        let sentence = match status.as_u16() {
            401 => "Anthropic rejected the API key".to_string(),
            429 => "Rate limited by Anthropic".to_string(),
            _ => format!("Anthropic API error {status}"),
        };
        return Err(match detail {
            Some(detail) => format!("{sentence}: {detail}"),
            None => sentence,
        });
    }

    let parsed: Response = serde_json::from_str(&body)
        .map_err(|e| format!("unexpected Anthropic response shape: {e}"))?;

    let text = parsed
        .content
        .iter()
        .filter(|b| b.kind == "text")
        .filter_map(|b| b.text.as_deref())
        .collect::<Vec<_>>()
        .join("\n\n");
    let refused = parsed.stop_reason.as_deref() == Some("refusal");
    let truncated = parsed.stop_reason.as_deref() == Some("max_tokens");
    let text = if text.trim().is_empty() && refused {
        "Claude declined to answer that one.".to_string()
    } else {
        text
    };

    Ok(ChatReply {
        text: with_truncation_note(text, truncated),
        // Not asked for and not rendered: see `chat_request.rs`.
        thinking: None,
        model: parsed.model,
        refused,
        truncated,
        input_tokens: parsed.usage.input_tokens,
        output_tokens: parsed.usage.output_tokens,
        cache_creation_input_tokens: parsed.usage.cache_creation_input_tokens,
        cache_read_input_tokens: parsed.usage.cache_read_input_tokens,
        provider: String::new(),
        cost_usd: 0.0,
        screen_spend_usd: 0.0,
    })
}

/// Minimal blocking helper so the tests here need no async runtime crate.
#[cfg(test)]
fn tokio_test_block<F: std::future::Future>(future: F) -> F::Output {
    tokio::runtime::Builder::new_current_thread()
        .enable_all()
        .build()
        .expect("runtime")
        .block_on(future)
}

/// Response parsing against a real socket. Its own file only because this one
/// is at the line cap.
#[cfg(test)]
#[path = "chat_wire_tests.rs"]
mod wire_tests;

/// The request shape and the pure helpers around it. Its own file only
/// because this one is at the line cap.
#[cfg(test)]
#[path = "chat_request_tests.rs"]
mod request_tests;
