//! One answer assembled out of the Messages API's server-sent events.
//!
//! The request streams (see `MAX_TOKENS` in `chat_request.rs`), so what
//! arrives is not a message but a run of events: `message_start` with the
//! model and the prompt-side usage, `content_block_delta`s carrying the text a
//! few words at a time, and `message_delta` with the stop reason and the
//! output-side usage. This turns those back into the one message the rest of
//! the chat code expects. Nothing here is rendered as it arrives; the panel
//! still gets a finished answer.

use serde::Deserialize;
use std::collections::BTreeMap;

/// The token counts a turn is billed on, as they arrive across the stream.
#[derive(Deserialize, Default, Clone, Debug, PartialEq, Eq)]
pub struct Usage {
    #[serde(default)]
    pub input_tokens: u32,
    #[serde(default)]
    pub output_tokens: u32,
    /// Absent on a response that used no cache at all, hence the defaults.
    #[serde(default)]
    pub cache_creation_input_tokens: u32,
    #[serde(default)]
    pub cache_read_input_tokens: u32,
}

impl Usage {
    /// Fold in a later report. Every count the API sends is cumulative, and
    /// a later event may omit a field an earlier one carried, so each is
    /// kept at the largest value seen rather than overwritten.
    fn absorb(&mut self, later: &Usage) {
        self.input_tokens = self.input_tokens.max(later.input_tokens);
        self.output_tokens = self.output_tokens.max(later.output_tokens);
        self.cache_creation_input_tokens = self
            .cache_creation_input_tokens
            .max(later.cache_creation_input_tokens);
        self.cache_read_input_tokens = self
            .cache_read_input_tokens
            .max(later.cache_read_input_tokens);
    }
}

/// A finished answer: what a non-streaming response used to say.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Answer {
    pub model: String,
    /// The text blocks, joined with a blank line. Thinking blocks are not in
    /// it; nothing renders them.
    pub text: String,
    pub stop_reason: Option<String>,
    /// `stop_details.category` when the stop reason is a refusal: which
    /// safety classifier declined, if the API said.
    pub stop_category: Option<String>,
    pub usage: Usage,
}

// ---------- event wire types ----------

#[derive(Deserialize)]
#[serde(tag = "type")]
enum Event {
    #[serde(rename = "message_start")]
    MessageStart { message: StartMessage },
    #[serde(rename = "content_block_start")]
    ContentBlockStart { index: usize, content_block: Block },
    #[serde(rename = "content_block_delta")]
    ContentBlockDelta { index: usize, delta: Delta },
    #[serde(rename = "message_delta")]
    MessageDelta {
        delta: MessageDelta,
        #[serde(default)]
        usage: Option<Usage>,
    },
    #[serde(rename = "error")]
    Error { error: ErrorDetail },
    /// `content_block_stop`, `message_stop`, `ping`, and anything added
    /// later. Read past: none of them carry text or money.
    #[serde(other)]
    Other,
}

#[derive(Deserialize)]
struct StartMessage {
    #[serde(default)]
    model: String,
    #[serde(default)]
    usage: Usage,
}

#[derive(Deserialize)]
struct Block {
    #[serde(rename = "type")]
    kind: String,
    #[serde(default)]
    text: String,
}

#[derive(Deserialize)]
struct Delta {
    #[serde(rename = "type")]
    kind: String,
    #[serde(default)]
    text: String,
}

#[derive(Deserialize)]
struct MessageDelta {
    #[serde(default)]
    stop_reason: Option<String>,
    #[serde(default)]
    stop_details: Option<StopDetails>,
}

#[derive(Deserialize)]
struct StopDetails {
    #[serde(default)]
    category: Option<String>,
}

#[derive(Deserialize)]
struct ErrorDetail {
    #[serde(default)]
    message: String,
}

/// The answer so far, fed the body a chunk at a time.
#[derive(Default, Debug)]
pub struct Stream {
    /// Bytes of an event that has not finished arriving.
    buffer: Vec<u8>,
    started: bool,
    model: String,
    /// Text blocks by index. Only text: a thinking block's deltas are read
    /// past, as the non-streaming reply's thinking blocks were.
    texts: BTreeMap<usize, String>,
    stop_reason: Option<String>,
    stop_category: Option<String>,
    usage: Usage,
}

impl Stream {
    pub fn new() -> Self {
        Self::default()
    }

    /// True once `message_start` has arrived: the request was accepted and
    /// is being billed, whatever happens to the rest of the stream.
    pub fn started(&self) -> bool {
        self.started
    }

    pub fn model(&self) -> &str {
        &self.model
    }

    /// What the stream has said it is charging so far.
    pub fn usage(&self) -> &Usage {
        &self.usage
    }

    /// Take in the next chunk of the body and act on every event that is now
    /// complete. Events end at a blank line; a chunk can end anywhere.
    pub fn feed(&mut self, bytes: &[u8]) -> Result<(), String> {
        self.buffer.extend_from_slice(bytes);
        while let Some((end, gap)) = event_end(&self.buffer) {
            let raw: Vec<u8> = self.buffer.drain(..end + gap).collect();
            self.event(&String::from_utf8_lossy(&raw[..end]))?;
        }
        Ok(())
    }

    /// The body has ended. Whatever is left in the buffer is one last event
    /// without its blank line.
    pub fn finish(mut self) -> Result<Answer, String> {
        if !self.buffer.is_empty() {
            let raw = std::mem::take(&mut self.buffer);
            self.event(&String::from_utf8_lossy(&raw))?;
        }
        if !self.started {
            return Err("the Anthropic stream ended before the answer began".to_string());
        }
        Ok(Answer {
            model: self.model,
            text: self
                .texts
                .into_values()
                .filter(|t| !t.is_empty())
                .collect::<Vec<_>>()
                .join("\n\n"),
            stop_reason: self.stop_reason,
            stop_category: self.stop_category,
            usage: self.usage,
        })
    }

    fn event(&mut self, raw: &str) -> Result<(), String> {
        // Per the SSE spec, an event's data is its `data:` lines joined with
        // newlines; `event:` names it, `:` opens a comment, and both are
        // skipped. The JSON carries its own `type`, so the name is not needed.
        // A line that is none of those is not an event at all: a proxy's
        // page, or a plain JSON message from a server that ignored `stream`.
        // Naming that is more use than "the stream ended before it began".
        if let Some(line) = raw.lines().find(|line| {
            let line = line.trim_end_matches('\r');
            !(line.is_empty()
                || line.starts_with(':')
                || ["data:", "event:", "id:", "retry:"]
                    .iter()
                    .any(|field| line.starts_with(field)))
        }) {
            let shown: String = line.chars().take(60).collect();
            return Err(format!(
                "unexpected Anthropic stream event: not server-sent events ({shown})"
            ));
        }
        let data = raw
            .lines()
            .filter_map(|line| line.strip_prefix("data:"))
            .map(|d| d.strip_prefix(' ').unwrap_or(d))
            .collect::<Vec<_>>()
            .join("\n");
        if data.trim().is_empty() {
            return Ok(());
        }
        let event: Event = serde_json::from_str(&data)
            .map_err(|e| format!("unexpected Anthropic stream event: {e}"))?;
        match event {
            Event::MessageStart { message } => {
                self.started = true;
                self.model = message.model;
                self.usage.absorb(&message.usage);
            }
            Event::ContentBlockStart {
                index,
                content_block,
            } if content_block.kind == "text" => {
                self.texts.insert(index, content_block.text);
            }
            Event::ContentBlockDelta { index, delta } if delta.kind == "text_delta" => {
                self.texts.entry(index).or_default().push_str(&delta.text);
            }
            Event::MessageDelta { delta, usage } => {
                if delta.stop_reason.is_some() {
                    self.stop_reason = delta.stop_reason;
                }
                if let Some(category) = delta.stop_details.and_then(|d| d.category) {
                    self.stop_category = Some(category);
                }
                if let Some(usage) = usage {
                    self.usage.absorb(&usage);
                }
            }
            // An error event ends the stream: the answer so far is not one.
            Event::Error { error } => {
                return Err(format!("Anthropic API error: {}", error.message));
            }
            Event::ContentBlockStart { .. } | Event::ContentBlockDelta { .. } | Event::Other => {}
        }
        Ok(())
    }
}

/// Where the first complete event in `bytes` ends: `(length, gap)`, the gap
/// being the blank line that closes it, in either line-ending style.
fn event_end(bytes: &[u8]) -> Option<(usize, usize)> {
    let lf = bytes.windows(2).position(|w| w == b"\n\n");
    let crlf = bytes.windows(4).position(|w| w == b"\r\n\r\n");
    match (lf, crlf) {
        (Some(a), Some(b)) if b < a => Some((b, 4)),
        (Some(a), _) => Some((a, 2)),
        (None, Some(b)) => Some((b, 4)),
        (None, None) => None,
    }
}

#[cfg(test)]
#[path = "chat_stream_tests.rs"]
mod tests;
