//! The "Ask Claude" panel's backend: the Anthropic Messages API over raw HTTP.
//!
//! Rust has no official Anthropic SDK, so this speaks the wire format
//! directly. The board, roster and clock are passed as a cached system prompt
//! rather than pasted into every user turn, so a long conversation re-sends
//! the volatile part only.

use serde::{Deserialize, Serialize};
use std::collections::HashMap;

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
    fn can_disable_thinking(self) -> bool {
        matches!(self, ChatModel::Opus5)
    }
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
    betas: Vec<&'static str>,
    /// Route around a policy decline instead of returning nothing.
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

/// The instruction the panel operates under. Kept separate from the volatile
/// board state so the stable half caches.
pub(crate) const GUIDANCE: &str = "\
You are a fantasy football draft and season assistant embedded in a read-only \
Sleeper second-screen app. You can see the user's live board, roster, and \
clock in the context below.

Answer in two or three short paragraphs at most. Lead with the recommendation, \
then the reasoning, then the risk. Cite the numbers you were given (points, \
VORP, tier, survival odds) rather than inventing any. If the context does not \
contain what you would need, say so plainly instead of guessing.

This app cannot draft, set a lineup, or write anything to Sleeper. Never tell \
the user you have done something for them; tell them what to do.

Do not include internal or system XML tags in your response.";

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
                text: GUIDANCE,
                // Everything above this point is byte-identical every turn.
                cache_control: Some(CacheControl { kind: "ephemeral" }),
            },
            SystemBlock {
                kind: "text",
                text: context,
                cache_control: None,
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
        betas: vec![FALLBACK_BETA],
        fallbacks: "default",
    };

    let response = http
        .post(ENDPOINT)
        .header("x-api-key", api_key)
        .header("anthropic-version", API_VERSION)
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
            .and_then(|b| b.error.and_then(|e| e.message))
            .unwrap_or_else(|| body.chars().take(300).collect());
        return Err(match status.as_u16() {
            401 => format!("Anthropic rejected the API key: {detail}"),
            429 => format!("Rate limited by Anthropic: {detail}"),
            _ => format!("Anthropic API error {status}: {detail}"),
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
    })
}

/// Model / effort pairs the UI is allowed to offer.
pub fn effort_levels(model: ChatModel) -> Vec<&'static str> {
    if model.can_disable_thinking() {
        vec!["Off", "Low", "Medium", "High", "xhigh", "Max"]
    } else {
        // Fable 5 thinks on every turn; there is no off.
        vec!["Low", "Medium", "High", "xhigh", "Max"]
    }
}

/// Per-level copy for the tooltips and the footer note.
pub fn effort_note(effort: Effort) -> (&'static str, &'static str) {
    match effort {
        Effort::Off => (
            "Adaptive thinking disabled — Claude answers without a reasoning pass",
            "no extended thinking",
        ),
        Effort::Low => (
            "Most efficient — significant token savings, some capability reduction",
            "low effort · fastest",
        ),
        Effort::Medium => (
            "Balanced — moderate token savings",
            "medium effort · balanced",
        ),
        Effort::High => (
            "Default — spends as many tokens as needed for excellent results",
            "high effort · the default",
        ),
        Effort::XHigh => (
            "For the hardest problems and long-horizon work",
            "xhigh effort · sustained reasoning",
        ),
        Effort::Max => (
            "No constraints on token spend — deepest analysis",
            "max effort · deepest, slowest",
        ),
    }
}

/// Redact everything but the tail of a key, for display.
pub fn mask_key(key: &str) -> String {
    let visible: String = key
        .chars()
        .rev()
        .take(4)
        .collect::<Vec<_>>()
        .into_iter()
        .rev()
        .collect();
    if key.len() <= 4 {
        return "····".to_string();
    }
    format!("····{visible}")
}

/// Extra headers, exposed for tests and for callers that log requests.
pub fn beta_headers() -> HashMap<&'static str, &'static str> {
    HashMap::from([("anthropic-beta", FALLBACK_BETA)])
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn model_ids_are_the_exact_published_strings() {
        assert_eq!(ChatModel::Opus5.id(), "claude-opus-5");
        assert_eq!(ChatModel::Fable5.id(), "claude-fable-5");
    }

    #[test]
    fn only_opus_offers_thinking_off() {
        assert!(effort_levels(ChatModel::Opus5).contains(&"Off"));
        assert!(!effort_levels(ChatModel::Fable5).contains(&"Off"));
    }

    #[test]
    fn effort_labels_map_to_api_values() {
        assert_eq!(Effort::parse("xhigh").api_effort(), "xhigh");
        assert_eq!(Effort::parse("Max").api_effort(), "max");
        assert_eq!(Effort::parse("nonsense").api_effort(), "high");
        // Disabled thinking must not ride at xhigh/max, which the API rejects.
        assert_eq!(Effort::Off.api_effort(), "medium");
    }

    #[test]
    fn an_empty_key_fails_before_any_request_is_made() {
        let http = reqwest::Client::new();
        let result = tokio_test_block(ask(
            &http,
            "   ",
            ChatModel::Opus5,
            Effort::High,
            "context",
            &[ChatMessage {
                role: "user".into(),
                content: "hi".into(),
            }],
        ));
        assert!(result.unwrap_err().contains("no Anthropic API key"));
    }

    #[test]
    fn keys_are_masked_to_their_last_four() {
        assert_eq!(mask_key("sk-ant-api03-abcd1234"), "····1234");
        assert_eq!(mask_key("abc"), "····");
    }

    #[test]
    fn the_request_body_matches_the_documented_wire_shape() {
        let messages = [ChatMessage {
            role: "user".into(),
            content: "Walker or Bowers?".into(),
        }];
        let request = Request {
            model: ChatModel::Opus5.id(),
            max_tokens: MAX_TOKENS,
            system: vec![SystemBlock {
                kind: "text",
                text: GUIDANCE,
                cache_control: Some(CacheControl { kind: "ephemeral" }),
            }],
            messages: &messages,
            output_config: OutputConfig { effort: "xhigh" },
            thinking: Some(Thinking {
                kind: "adaptive",
                display: Some("summarized"),
            }),
            betas: vec![FALLBACK_BETA],
            fallbacks: "default",
        };
        let json = serde_json::to_value(&request).unwrap();
        assert_eq!(json["model"], "claude-opus-5");
        assert_eq!(json["output_config"]["effort"], "xhigh");
        assert_eq!(json["thinking"]["type"], "adaptive");
        assert_eq!(json["fallbacks"], "default");
        // budget_tokens is removed on these models and 400s if sent.
        assert!(json["thinking"].get("budget_tokens").is_none());
        assert!(json.get("temperature").is_none());
    }

    /// Minimal blocking helper so these tests need no async runtime crate.
    fn tokio_test_block<F: std::future::Future>(future: F) -> F::Output {
        tokio::runtime::Builder::new_current_thread()
            .enable_all()
            .build()
            .expect("runtime")
            .block_on(future)
    }
}
