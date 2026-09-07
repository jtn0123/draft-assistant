//! The "Ask Claude" panel's backend: the Anthropic Messages API over raw HTTP.
//!
//! Rust has no official Anthropic SDK, so this speaks the wire format
//! directly. The board, roster and clock are passed as a system prompt rather
//! than pasted into every user turn.
//!
//! The request body, cache breakpoint included, is in `chat_request.rs`; what
//! is here is the call and everything that happens to the reply.

use std::sync::Arc;
use std::time::Duration;

/// The models, the effort levels, a turn, a reply and the error type.
#[path = "chat_types.rs"]
mod types;
pub(crate) use types::with_truncation_note;
pub use types::{ChatError, ChatMessage, ChatModel, ChatReply, Effort, TRUNCATED_NOTE};

pub use crate::chat_client::CancelSignal;

/// The sentence for a status, and the retry policy in front of it.
#[path = "chat_retry.rs"]
mod retry;

const ENDPOINT: &str = "https://api.anthropic.com/v1/messages";
const API_VERSION: &str = "2023-06-01";
/// Opt into server-side refusal fallbacks (the `fallbacks: "default"` form).
const FALLBACK_BETA: &str = "server-side-fallback-2026-07-01";

/// The request body's own types, its cache breakpoints and its ceilings.
#[path = "chat_request.rs"]
mod request;

/// The answer, reassembled out of the stream of events it arrives as.
#[path = "chat_stream.rs"]
pub mod stream;

use request::build_request;

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

/// The canned line for a refusal with no text, naming the safety category
/// when the API gave one. `stop_details` is informational — the category can
/// be absent even on a refusal — so the line reads the same without it.
pub(crate) fn refusal_text(category: Option<&str>) -> String {
    match category {
        Some(category) if !category.trim().is_empty() => {
            let category: String = category.trim().chars().take(40).collect();
            format!("Claude declined to answer that one (safety category: {category}).")
        }
        _ => "Claude declined to answer that one.".to_string(),
    }
}

/// A reply with no text, from what the stream had said before it stopped.
fn partial_reply(stream: &stream::Stream) -> Option<Box<ChatReply>> {
    if !stream.started() {
        return None;
    }
    Some(Box::new(reply_from(
        stream.model().to_string(),
        String::new(),
        stream.usage(),
        Flags::default(),
    )))
}

/// The answer the user stopped: the text that had arrived, the usage the
/// stream had reported, and the flag the panel marks it cut short by. Not an
/// error, because a stopped answer is still one the user asked for and still
/// one the API bills for from `message_start` on.
fn cut_short(stream: &stream::Stream) -> ChatReply {
    reply_from(
        stream.model().to_string(),
        stream.text_so_far(),
        stream.usage(),
        Flags {
            cancelled: true,
            ..Flags::default()
        },
    )
}

/// How an answer ended, other than plainly.
#[derive(Default, Clone, Copy)]
struct Flags {
    refused: bool,
    truncated: bool,
    cancelled: bool,
}

fn reply_from(model: String, text: String, usage: &stream::Usage, flags: Flags) -> ChatReply {
    ChatReply {
        text,
        // Not asked for and not rendered: see `chat_request.rs`.
        thinking: None,
        model,
        refused: flags.refused,
        truncated: flags.truncated,
        cancelled: flags.cancelled,
        input_tokens: usage.input_tokens,
        output_tokens: usage.output_tokens,
        cache_creation_input_tokens: usage.cache_creation_input_tokens,
        cache_read_input_tokens: usage.cache_read_input_tokens,
        provider: String::new(),
        cost_usd: 0.0,
        screen_spend_usd: 0.0,
    }
}

/// Ask Claude about the current board.
///
/// `context` is the serialized view (draft or season) the panel is showing,
/// in the two halves the request builder places: the stable half in the
/// cached system prompt, the board after the conversation. `cancel` is the
/// signal the panel's Cancel button pulls; a stopped answer comes back as a
/// reply marked `cancelled`, with whatever text had arrived.
pub async fn ask(
    http: &reqwest::Client,
    api_key: &str,
    model: ChatModel,
    effort: Effort,
    context: &crate::chat_context::SplitContext,
    messages: &[ChatMessage],
    cancel: Arc<CancelSignal>,
) -> Result<ChatReply, ChatError> {
    let call = Call {
        endpoint: ENDPOINT,
        http,
        api_key,
        model,
        effort,
        context,
        messages,
    };
    ask_at(call, cancel, retry::BASE_BACKOFF).await
}

/// One question as the wire sees it: where it goes, what it says, and the
/// key that signs it.
struct Call<'a> {
    endpoint: &'a str,
    http: &'a reqwest::Client,
    api_key: &'a str,
    model: ChatModel,
    effort: Effort,
    context: &'a crate::chat_context::SplitContext,
    messages: &'a [ChatMessage],
}

/// The same request against an arbitrary endpoint, with the retry backoff
/// passed in. Only [`ask`] and the wire tests, which point it at a stub server
/// and cannot wait seconds between attempts, call this.
async fn ask_at(
    call: Call<'_>,
    cancel: Arc<CancelSignal>,
    backoff: Duration,
) -> Result<ChatReply, ChatError> {
    let Call {
        endpoint,
        http,
        api_key,
        model,
        effort,
        context,
        messages,
    } = call;
    if api_key.trim().is_empty() {
        return Err("no Anthropic API key set — add one in Settings"
            .to_string()
            .into());
    }
    if messages.is_empty() {
        return Err("nothing to ask".to_string().into());
    }
    // The board goes after the thread as a system-role message, which must
    // follow a user turn. A thread ending on the assistant's turn is asking
    // the model to continue its own answer, which these models refuse anyway.
    if messages.last().is_some_and(|m| m.role != "user") {
        return Err("the last turn must be a question".to_string().into());
    }

    let request = build_request(model, effort, context, messages);
    let sent_at = std::time::Instant::now();
    let send = || {
        http.post(endpoint)
            .header("x-api-key", api_key)
            .header("anthropic-version", API_VERSION)
            .header("anthropic-beta", FALLBACK_BETA)
            .header("content-type", "application/json")
            .header("accept", "text/event-stream")
            .json(&request)
            .send()
    };

    // A 429, a 529 or a 5xx is "not right now": the same request is sent
    // again after a pause, twice, before its status is shown. Nothing is
    // billed until a 200 begins, so a retry never pays for the attempt before.
    let mut attempt = 1;
    let mut response = loop {
        if cancel.is_cancelled() {
            return Ok(cut_short(&stream::Stream::new()));
        }
        let response = send()
            .await
            .map_err(|e| format!("could not reach the Anthropic API: {e}"))?;
        let status = response.status();
        if status.is_success() {
            break response;
        }
        let retry_after = retry::retry_after(response.headers());
        let body = response
            .text()
            .await
            .map_err(|e| format!("could not read the Anthropic response: {e}"))?;
        let message = retry::error_message(status, &body);
        let Some(wait) = retry::delay_before(status, attempt + 1, retry_after, backoff) else {
            return Err(retry::gave_up(message, attempt).into());
        };
        crate::applog::warn(format!(
            "Anthropic API {}: retrying in {}ms (attempt {} of {})",
            status.as_u16(),
            wait.as_millis(),
            attempt + 1,
            retry::MAX_ATTEMPTS
        ));
        tokio::select! {
            () = tokio::time::sleep(wait) => {}
            () = cancel.cancelled() => return Ok(cut_short(&stream::Stream::new())),
        }
        attempt += 1;
    };

    // Read the events as they arrive. A body that stops early — the client's
    // own timeout, or the socket dropping — is still a billed request from
    // `message_start` on, so the error carries what had been charged so far.
    // A cancel drops the response, which closes the request, and hands back
    // what had arrived.
    let mut stream = stream::Stream::new();
    loop {
        let chunk = tokio::select! {
            chunk = response.chunk() => chunk,
            () = cancel.cancelled() => return Ok(cut_short(&stream)),
        };
        match chunk {
            Ok(Some(bytes)) => {
                if let Err(message) = stream.feed(&bytes) {
                    return Err(ChatError {
                        partial: partial_reply(&stream),
                        message,
                    });
                }
            }
            Ok(None) => break,
            Err(e) => {
                return Err(ChatError {
                    partial: partial_reply(&stream),
                    message: format!("the Anthropic answer stopped early: {e}"),
                });
            }
        }
    }
    let answer = stream.finish()?;

    // Model, effort, token counts and the round trip. Never the prompt or the
    // answer: the log is for pasting into a chat window.
    crate::applog::debug(format!(
        "chat answered model={} effort={} in={} out={} cache_read={} stop={} in {}ms",
        answer.model,
        effort.api_effort(),
        answer.usage.input_tokens,
        answer.usage.output_tokens,
        answer.usage.cache_read_input_tokens,
        answer.stop_reason.as_deref().unwrap_or("none"),
        sent_at.elapsed().as_millis()
    ));
    let flags = Flags {
        refused: answer.stop_reason.as_deref() == Some("refusal"),
        truncated: answer.stop_reason.as_deref() == Some("max_tokens"),
        cancelled: false,
    };
    let text = if answer.text.trim().is_empty() && flags.refused {
        refusal_text(answer.stop_category.as_deref())
    } else {
        answer.text
    };
    Ok(reply_from(
        answer.model,
        with_truncation_note(text, flags.truncated),
        &answer.usage,
        flags,
    ))
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

/// The stub server and event builders the wire tests share.
#[cfg(test)]
#[path = "chat_wire_stub.rs"]
mod wire_stub;

/// Response parsing against a real socket. Its own file only because this one
/// is at the line cap.
#[cfg(test)]
#[path = "chat_wire_tests.rs"]
mod wire_tests;

/// Retries and cancellation against the same stub server.
#[cfg(test)]
#[path = "chat_wire_retry_tests.rs"]
mod wire_retry_tests;

/// The request shape and the pure helpers around it. Its own file only
/// because this one is at the line cap.
#[cfg(test)]
#[path = "chat_request_tests.rs"]
mod request_tests;
