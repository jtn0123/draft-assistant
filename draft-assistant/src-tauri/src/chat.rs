//! The "Ask Claude" panel's backend: the Anthropic Messages API over raw HTTP.
//!
//! Rust has no official Anthropic SDK, so this speaks the wire format
//! directly. The board, roster and clock are passed as a system prompt rather
//! than pasted into every user turn.
//!
//! The cache breakpoint sits on the *last* system block, so the cached prefix
//! is the guidance plus the context together. The guidance alone is around 450
//! tokens — under the 512-token minimum a prefix must reach before Opus 5
//! caches it at all — so a breakpoint on the guidance by itself silently
//! cached nothing. Caching is a byte-exact prefix match, so the hits are real
//! only while the context is unchanged: a second question about the same board
//! reads the prefix back, and the next pick rewrites it.

use serde::{Deserialize, Serialize};

const ENDPOINT: &str = "https://api.anthropic.com/v1/messages";
const API_VERSION: &str = "2023-06-01";
/// Opt into server-side refusal fallbacks (the `fallbacks: "default"` form).
const FALLBACK_BETA: &str = "server-side-fallback-2026-07-01";
const MAX_TOKENS: u32 = 16000;

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
    /// Present when the model summarised its reasoning.
    pub thinking: Option<String>,
    pub model: String,
    /// True when safety classifiers declined and no fallback rescued it.
    pub refused: bool,
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

// ---------- request wire types ----------

#[derive(Serialize)]
struct SystemBlock<'a> {
    #[serde(rename = "type")]
    kind: &'static str,
    text: &'a str,
    #[serde(skip_serializing_if = "Option::is_none")]
    cache_control: Option<CacheControl>,
}

#[derive(Serialize)]
struct CacheControl {
    #[serde(rename = "type")]
    kind: &'static str,
}

#[derive(Serialize)]
struct Thinking {
    #[serde(rename = "type")]
    kind: &'static str,
    #[serde(skip_serializing_if = "Option::is_none")]
    display: Option<&'static str>,
}

#[derive(Serialize)]
struct OutputConfig {
    effort: &'static str,
}

#[derive(Serialize)]
struct Request<'a> {
    model: &'a str,
    max_tokens: u32,
    system: Vec<SystemBlock<'a>>,
    messages: &'a [ChatMessage],
    output_config: OutputConfig,
    #[serde(skip_serializing_if = "Option::is_none")]
    thinking: Option<Thinking>,
    /// Route around a policy decline instead of returning nothing. A body
    /// parameter; the beta that enables it travels in the `anthropic-beta`
    /// header, which is the only place the API looks for it.
    fallbacks: &'static str,
}

// ---------- response wire types ----------

#[derive(Deserialize)]
struct ContentBlock {
    #[serde(rename = "type")]
    kind: String,
    #[serde(default)]
    text: Option<String>,
    #[serde(default)]
    thinking: Option<String>,
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
/// `context` is the serialized view (draft or season) the panel is showing.
pub async fn ask(
    http: &reqwest::Client,
    api_key: &str,
    model: ChatModel,
    effort: Effort,
    context: &str,
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
    context: &str,
    messages: &[ChatMessage],
) -> Result<ChatReply, String> {
    if api_key.trim().is_empty() {
        return Err("no Anthropic API key set — add one in Settings".into());
    }
    if messages.is_empty() {
        return Err("nothing to ask".into());
    }

    let disable_thinking = effort == Effort::Off && model.can_disable_thinking();
    let request = Request {
        model: model.id(),
        max_tokens: MAX_TOKENS,
        system: vec![
            SystemBlock {
                kind: "text",
                text: crate::chat_copy::GUIDANCE,
                cache_control: None,
            },
            SystemBlock {
                kind: "text",
                // Guidance + context is the cacheable prefix: the guidance on
                // its own is too short to reach the minimum. Everything the
                // user types renders after this block, so a second question
                // about an unchanged board reads the whole prefix back.
                text: context,
                cache_control: Some(CacheControl { kind: "ephemeral" }),
            },
        ],
        messages,
        output_config: OutputConfig {
            effort: effort.api_effort(),
        },
        thinking: if disable_thinking {
            Some(Thinking {
                kind: "disabled",
                display: None,
            })
        } else {
            Some(Thinking {
                kind: "adaptive",
                display: Some("summarized"),
            })
        },
        fallbacks: "default",
    };

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
    let thinking = parsed
        .content
        .iter()
        .filter(|b| b.kind == "thinking")
        .filter_map(|b| b.thinking.as_deref())
        .filter(|t| !t.trim().is_empty())
        .collect::<Vec<_>>()
        .join("\n\n");
    let refused = parsed.stop_reason.as_deref() == Some("refusal");

    Ok(ChatReply {
        text: if text.trim().is_empty() && refused {
            "Claude declined to answer that one.".to_string()
        } else {
            text
        },
        thinking: if thinking.is_empty() {
            None
        } else {
            Some(thinking)
        },
        model: parsed.model,
        refused,
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
