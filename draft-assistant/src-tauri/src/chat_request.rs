//! The body `chat.rs` posts to the Messages API.
//!
//! Its own file so `chat.rs` stays inside the line cap, and because the shape
//! of this request is the part worth reading on its own: where the cache
//! breakpoint sits, and what is deliberately not asked for.

use crate::chat::{ChatMessage, ChatModel, Effort};
use crate::chat_context::SplitContext;
use serde::Serialize;

/// The output ceiling for one answer.
///
/// 16,000 is the documented default for a *non-streaming* request: anything
/// much larger risks the answer taking longer than the HTTP timeout, and this
/// route does not stream. Thinking tokens are billed against this same
/// ceiling, which is why the note on a cut-off answer suggests a lower effort
/// as well as a shorter question.
pub const MAX_TOKENS: u32 = 16000;

#[derive(Serialize)]
pub struct SystemBlock<'a> {
    #[serde(rename = "type")]
    pub kind: &'static str,
    pub text: &'a str,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub cache_control: Option<CacheControl>,
}

#[derive(Serialize)]
pub struct CacheControl {
    #[serde(rename = "type")]
    pub kind: &'static str,
}

#[derive(Serialize)]
pub struct Thinking {
    #[serde(rename = "type")]
    pub kind: &'static str,
}

#[derive(Serialize)]
pub struct OutputConfig {
    pub effort: &'static str,
}

#[derive(Serialize)]
pub struct Request<'a> {
    pub model: &'a str,
    pub max_tokens: u32,
    pub system: Vec<SystemBlock<'a>>,
    pub messages: &'a [ChatMessage],
    pub output_config: OutputConfig,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub thinking: Option<Thinking>,
    /// Route around a policy decline instead of returning nothing. A body
    /// parameter; the beta that enables it travels in the `anthropic-beta`
    /// header, which is the only place the API looks for it.
    pub fallbacks: &'static str,
}

/// The system prompt as blocks, with the breakpoint on the last stable one.
///
/// The API stores a cached prefix only once it is long enough — a token count
/// that depends on the model, 1,024 on both models this panel offers and 2,048
/// on the largest published tier. Tokens cannot be counted here without a
/// second API call, so the target is in characters: prose and board rows of
/// this kind run about four characters to the token, so the target is 4,096
/// characters and up. The guidance and a forty-player board measure around
/// 5,200 together, which clears the 1,024-token minimum with room to spare;
/// nothing here would reach a 2,048-token one, so a model with that minimum
/// would need the board lengthened before its cache did anything. The test
/// beside this file measures it, so a summariser trimmed too far shows up as
/// a failure rather than as a cache that quietly stopped storing anything.
///
/// Order matters and is the whole point: the guidance never changes, the
/// league and the board change when the board does, and the clock changes on
/// every pick. A breakpoint after the first two caches the long half; the
/// clock renders after it, where rewriting it costs nothing. The breakpoint
/// used to sit on the block that carried the pick number, so every pick threw
/// the cached prefix away and each question paid the 1.25x write again.
pub fn system_blocks(context: &SplitContext) -> Vec<SystemBlock<'_>> {
    let mut blocks = vec![
        SystemBlock {
            kind: "text",
            text: crate::chat_copy::GUIDANCE,
            cache_control: None,
        },
        SystemBlock {
            kind: "text",
            text: &context.stable,
            cache_control: Some(CacheControl { kind: "ephemeral" }),
        },
    ];
    if !context.volatile.is_empty() {
        blocks.push(SystemBlock {
            kind: "text",
            text: &context.volatile,
            cache_control: None,
        });
    }
    blocks
}

/// The whole body for one turn.
pub fn build_request<'a>(
    model: ChatModel,
    effort: Effort,
    context: &'a SplitContext,
    messages: &'a [ChatMessage],
) -> Request<'a> {
    let disable_thinking = effort == Effort::Off && model.can_disable_thinking();
    Request {
        model: model.id(),
        max_tokens: MAX_TOKENS,
        system: system_blocks(context),
        messages,
        output_config: OutputConfig {
            effort: effort.api_effort(),
        },
        // No `display`. Summarised thinking is a readable version of the
        // reasoning, billed as output tokens, and nothing in this app ever put
        // it on screen — so it was paid for on every turn and thrown away. The
        // thinking itself still happens; only the summary is not asked for.
        thinking: Some(Thinking {
            kind: if disable_thinking {
                "disabled"
            } else {
                "adaptive"
            },
        }),
        fallbacks: "default",
    }
}
