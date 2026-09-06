//! The body `chat.rs` posts to the Messages API.
//!
//! Its own file so `chat.rs` stays inside the line cap, and because the shape
//! of this request is the part worth reading on its own: where the cache
//! breakpoints sit, where the board goes, and what is deliberately not asked
//! for.

use crate::chat::{ChatMessage, ChatModel, Effort};
use crate::chat_context::SplitContext;
use serde::Serialize;

/// The output ceiling for one answer.
///
/// Thinking is billed against this same ceiling as the answer, and at a high
/// effort it can run to tens of thousands of tokens on its own. The old
/// ceiling of 16,000 was the documented default for a request that does not
/// stream — it had to fit under the HTTP timeout — and at xhigh it cut real
/// answers off mid-thought. The request streams now, so the timeout is not a
/// constraint on length, and 64,000 is the documented default for a streaming
/// request: room for the thinking and the answer both.
pub const MAX_TOKENS: u32 = 64000;

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

/// One text block of a message's content.
#[derive(Serialize)]
pub struct TextBlock<'a> {
    #[serde(rename = "type")]
    pub kind: &'static str,
    pub text: &'a str,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub cache_control: Option<CacheControl>,
}

/// A message's content: a bare string, or blocks when one of them needs a
/// cache breakpoint.
#[derive(Serialize)]
#[serde(untagged)]
pub enum Content<'a> {
    Text(&'a str),
    Blocks(Vec<TextBlock<'a>>),
}

/// A message as the API reads it. The panel's [`ChatMessage`] is a role and
/// a string; this is the same thing with room for a breakpoint, and for the
/// system-role message that carries the board.
#[derive(Serialize)]
pub struct WireMessage<'a> {
    pub role: &'a str,
    pub content: Content<'a>,
}

#[derive(Serialize)]
pub struct Request<'a> {
    pub model: &'a str,
    pub max_tokens: u32,
    /// Always true. The answer arrives as server-sent events and is assembled
    /// in `chat_stream.rs`; see [`MAX_TOKENS`] for why.
    pub stream: bool,
    pub system: Vec<SystemBlock<'a>>,
    pub messages: Vec<WireMessage<'a>>,
    pub output_config: OutputConfig,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub thinking: Option<Thinking>,
    /// Route around a policy decline instead of returning nothing. A body
    /// parameter; the beta that enables it travels in the `anthropic-beta`
    /// header, which is the only place the API looks for it.
    pub fallbacks: &'static str,
}

/// The system prompt as blocks, with the breakpoint on the last one.
///
/// Both blocks are fixed for the length of a draft: the guidance never
/// changes, and `stable` is the league, its scoring, its roster shape, its
/// house rules and the user's slot. Nothing a pick rewrites is allowed in
/// here — the board used to be, and every pick threw the cached prefix away.
///
/// The API stores a cached prefix only once it is long enough, a token count
/// that depends on the model (1,024 on both this panel offers). These two
/// blocks together run to about 1,900 characters, under 500 tokens, so on
/// their own they cache nothing; the breakpoint that does the work is the
/// one [`wire_messages`] puts on the last turn of the history, which covers
/// these blocks *and* the thread. The first question of a thread therefore
/// writes no cache and reads none; from the second on, the whole conversation
/// so far is read back at a tenth of the price, pick or no pick.
pub fn system_blocks(context: &SplitContext) -> Vec<SystemBlock<'_>> {
    vec![
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
    ]
}

/// The conversation as the API reads it, with the board after it.
///
/// The second breakpoint sits on the last turn of the history, so the whole
/// thread up to the new question is cached and the next question reads it
/// back. The board follows as a system-role message: an operator instruction
/// that arrives mid-conversation, which the API keeps out of the cached
/// prefix and reads with the system prompt's authority rather than the
/// user's. It must follow a user turn, and it does — `chat.rs` refuses a
/// thread that does not end on a question.
pub fn wire_messages<'a>(
    context: &'a SplitContext,
    messages: &'a [ChatMessage],
) -> Vec<WireMessage<'a>> {
    let last = messages.len().saturating_sub(1);
    let mut out: Vec<WireMessage<'a>> = messages
        .iter()
        .enumerate()
        .map(|(i, m)| WireMessage {
            role: &m.role,
            content: Content::Blocks(vec![TextBlock {
                kind: "text",
                text: &m.content,
                cache_control: (i == last).then_some(CacheControl { kind: "ephemeral" }),
            }]),
        })
        .collect();
    if !context.volatile.is_empty() {
        out.push(WireMessage {
            role: "system",
            content: Content::Text(&context.volatile),
        });
    }
    out
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
        stream: true,
        system: system_blocks(context),
        messages: wire_messages(context, messages),
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
