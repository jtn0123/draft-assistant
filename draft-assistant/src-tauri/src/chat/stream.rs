//! Reading what `claude --output-format stream-json` prints: one JSON object
//! per line. Text arrives as `stream_event` / `content_block_delta` /
//! `text_delta` chunks while the model writes; the final `result` line
//! carries the whole answer, the usage, and the cost. Thinking deltas, tool
//! events and status lines are skipped.

use serde::{Deserialize, Serialize};

/// What one call cost, as the CLI reports it. `context_tokens` is everything
/// the model read (fresh + cached input) — the number that tells the user how
/// big the thread has grown.
#[derive(Debug, Clone, Default, PartialEq, Serialize, Deserialize)]
pub struct ChatUsage {
    pub model: String,
    pub input_tokens: u64,
    pub cache_read_tokens: u64,
    pub cache_write_tokens: u64,
    pub output_tokens: u64,
    pub context_tokens: u64,
    pub web_searches: u64,
    pub duration_ms: u64,
    pub cost_usd: Option<f64>,
    /// `active` / `off` as reported; `None` when the CLI did not say.
    pub fast_mode: Option<String>,
    pub fast_mode_reason: Option<String>,
}

#[derive(Deserialize, Default)]
struct ServerToolUse {
    #[serde(default)]
    web_search_requests: u64,
}

#[derive(Deserialize, Default)]
struct CliUsage {
    #[serde(default)]
    input_tokens: u64,
    #[serde(default)]
    cache_creation_input_tokens: u64,
    #[serde(default)]
    cache_read_input_tokens: u64,
    #[serde(default)]
    output_tokens: u64,
    #[serde(default)]
    server_tool_use: ServerToolUse,
}

/// One content block of an assistant message. Only `tool_use` matters here.
#[derive(Deserialize, Default)]
struct Block {
    #[serde(default, rename = "type")]
    kind: String,
    #[serde(default)]
    name: String,
}

#[derive(Deserialize, Default)]
struct Message {
    #[serde(default)]
    content: Vec<Block>,
}

#[derive(Deserialize, Default)]
struct Delta {
    #[serde(default, rename = "type")]
    kind: String,
    #[serde(default)]
    text: String,
}

#[derive(Deserialize, Default)]
struct Event {
    #[serde(default, rename = "type")]
    kind: String,
    #[serde(default)]
    delta: Delta,
}

/// One line of the stream. Only the fields this app reads are named; the
/// CLI prints a great deal more.
#[derive(Deserialize, Default)]
struct Line {
    #[serde(default, rename = "type")]
    kind: String,
    #[serde(default)]
    event: Event,
    #[serde(default)]
    result: String,
    #[serde(default)]
    is_error: bool,
    #[serde(default)]
    duration_ms: u64,
    #[serde(default)]
    total_cost_usd: Option<f64>,
    #[serde(default)]
    usage: CliUsage,
    #[serde(default)]
    message: Message,
    #[serde(default)]
    fast_mode_state: Option<String>,
    #[serde(default)]
    fast_mode_disabled_reason: Option<String>,
}

/// What a line of the stream means to the panel.
#[derive(Debug, PartialEq)]
pub enum StreamLine {
    /// A piece of the answer, in order.
    Text(String),
    /// The final line: the complete answer and what it cost.
    Done { answer: String, usage: ChatUsage },
    /// The CLI reported a failure (not logged in, refused, ...).
    Failed(String),
    /// The model ran a web search. Counted here because the CLI runs
    /// WebSearch as a client tool, so `server_tool_use.web_search_requests`
    /// stays 0 however many searches really happened.
    Searched,
    /// Status, thinking, tool traffic — nothing the panel shows.
    Other,
}

/// Classify one line. Blank lines and lines that are not JSON are `Other`:
/// the CLI's own warnings land on stdout occasionally and must not abort a
/// stream that is otherwise fine.
pub fn parse_line(line: &str, model: &str) -> StreamLine {
    let trimmed = line.trim();
    if trimmed.is_empty() {
        return StreamLine::Other;
    }
    let Ok(parsed) = serde_json::from_str::<Line>(trimmed) else {
        return StreamLine::Other;
    };
    match parsed.kind.as_str() {
        "assistant"
            if parsed
                .message
                .content
                .iter()
                .any(|b| b.kind == "tool_use" && b.name == "WebSearch") =>
        {
            StreamLine::Searched
        }
        "stream_event" => {
            if parsed.event.kind == "content_block_delta" && parsed.event.delta.kind == "text_delta"
            {
                StreamLine::Text(parsed.event.delta.text)
            } else {
                StreamLine::Other
            }
        }
        "result" => {
            let answer = parsed.result.trim().to_string();
            if parsed.is_error {
                let detail = if answer.is_empty() {
                    "no detail".to_string()
                } else {
                    answer
                };
                return StreamLine::Failed(format!("Claude CLI error: {detail}"));
            }
            let u = parsed.usage;
            StreamLine::Done {
                answer,
                usage: ChatUsage {
                    model: model.to_string(),
                    input_tokens: u.input_tokens,
                    cache_read_tokens: u.cache_read_input_tokens,
                    cache_write_tokens: u.cache_creation_input_tokens,
                    context_tokens: u.input_tokens
                        + u.cache_read_input_tokens
                        + u.cache_creation_input_tokens,
                    output_tokens: u.output_tokens,
                    // Filled in by the accumulator when the count is client-side.
                    web_searches: u.server_tool_use.web_search_requests,
                    duration_ms: parsed.duration_ms,
                    cost_usd: parsed.total_cost_usd,
                    fast_mode: parsed.fast_mode_state,
                    fast_mode_reason: parsed.fast_mode_disabled_reason,
                },
            }
        }
        _ => StreamLine::Other,
    }
}

/// Folds the lines of one stream, in order, into the answer and usage. The
/// `result` line's answer wins over the concatenated chunks when both exist;
/// the chunks stand in when the CLI ends without one (a killed process), so
/// the user keeps what was written.
pub struct Accumulator {
    model: String,
    streamed: String,
    saw_json: bool,
    head: String,
    searches: u64,
}

impl Accumulator {
    pub fn new(model: &str) -> Self {
        Self {
            model: model.to_string(),
            streamed: String::new(),
            saw_json: false,
            head: String::new(),
            searches: 0,
        }
    }

    /// Take one line. Text goes to `on_text` as it is met. `Some` ends the
    /// stream: the finished answer, or the CLI's own failure.
    pub fn push(
        &mut self,
        line: &str,
        on_text: &mut dyn FnMut(&str),
    ) -> Option<Result<(String, ChatUsage), String>> {
        if !self.saw_json && self.head.chars().count() < 160 {
            self.head.push_str(line.trim());
        }
        match parse_line(line, &self.model) {
            StreamLine::Text(text) => {
                self.saw_json = true;
                on_text(&text);
                self.streamed.push_str(&text);
                None
            }
            StreamLine::Searched => {
                self.saw_json = true;
                self.searches += 1;
                None
            }
            StreamLine::Done { answer, mut usage } => {
                if usage.web_searches == 0 {
                    usage.web_searches = self.searches;
                }
                let answer = if answer.is_empty() {
                    self.streamed.trim().to_string()
                } else {
                    answer
                };
                Some(if answer.is_empty() {
                    Err("Claude returned an empty answer — try again".into())
                } else {
                    Ok((answer, usage))
                })
            }
            StreamLine::Failed(detail) => Some(Err(detail)),
            StreamLine::Other => {
                self.saw_json |= line.trim_start().starts_with('{');
                None
            }
        }
    }

    /// The stream ended without a `result` line.
    pub fn finish(self) -> String {
        if !self.saw_json && !self.head.is_empty() {
            let head: String = self.head.chars().take(160).collect();
            return format!("unexpected Claude CLI output: {head}");
        }
        if self.streamed.trim().is_empty() {
            "Claude returned an empty answer — try again".into()
        } else {
            "Claude stopped before finishing — try again".into()
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    const DELTA: &str = r#"{"type":"stream_event","event":{"type":"content_block_delta","index":1,"delta":{"type":"text_delta","text":"Take "}},"session_id":"s"}"#;
    const THINKING: &str = r#"{"type":"stream_event","event":{"type":"content_block_delta","index":0,"delta":{"type":"thinking_delta","thinking":"hmm"}}}"#;
    const RESULT: &str = r#"{"type":"result","subtype":"success","is_error":false,"duration_ms":9120,
        "result":"  Take Chris Olave.  ","total_cost_usd":0.31,
        "usage":{"input_tokens":12000,"cache_creation_input_tokens":3000,"cache_read_input_tokens":15000,
                 "output_tokens":80,"server_tool_use":{"web_search_requests":2}},
        "fast_mode_state":"off","fast_mode_disabled_reason":"extra_usage_disabled"}"#;

    #[test]
    fn text_deltas_are_text_and_everything_else_is_noise() {
        assert_eq!(parse_line(DELTA, "opus"), StreamLine::Text("Take ".into()));
        assert_eq!(parse_line(THINKING, "opus"), StreamLine::Other);
        assert_eq!(
            parse_line(r#"{"type":"system","subtype":"init"}"#, "opus"),
            StreamLine::Other
        );
        assert_eq!(parse_line("", "opus"), StreamLine::Other);
        assert_eq!(parse_line("not json at all", "opus"), StreamLine::Other);
    }

    #[test]
    fn the_result_line_yields_the_answer_and_usage() {
        let StreamLine::Done { answer, usage } = parse_line(RESULT, "opus") else {
            panic!("expected Done");
        };
        assert_eq!(answer, "Take Chris Olave.");
        assert_eq!(usage.context_tokens, 30000);
        assert_eq!(usage.output_tokens, 80);
        assert_eq!(usage.web_searches, 2);
        assert_eq!(usage.duration_ms, 9120);
        assert_eq!(usage.cost_usd, Some(0.31));
        assert_eq!(usage.fast_mode.as_deref(), Some("off"));
        assert_eq!(
            usage.fast_mode_reason.as_deref(),
            Some("extra_usage_disabled")
        );
        assert_eq!(usage.model, "opus");
    }

    #[test]
    fn a_client_side_web_search_is_counted_from_the_stream() {
        // The CLI runs WebSearch itself, so the result line reports zero
        // however many searches ran. Counting the tool_use lines is the only
        // honest number, and the usage line under the answer shows it.
        let mut acc = Accumulator::new("opus");
        let mut sink = |_: &str| {};
        let search = r#"{"type":"assistant","message":{"content":[
            {"type":"text","text":"I'll look."},
            {"type":"tool_use","name":"WebSearch","input":{"query":"nfl injuries"}}]}}"#;
        assert!(acc.push(search, &mut sink).is_none());
        assert!(acc.push(search, &mut sink).is_none());
        // A tool that is not a search does not count.
        let other =
            r#"{"type":"assistant","message":{"content":[{"type":"tool_use","name":"Read"}]}}"#;
        assert!(acc.push(other, &mut sink).is_none());
        let done = r#"{"type":"result","is_error":false,"result":"Two sources agree.",
            "duration_ms":900,"total_cost_usd":0.2,
            "usage":{"input_tokens":10,"output_tokens":5,"server_tool_use":{"web_search_requests":0}}}"#;
        let (answer, usage) = acc.push(done, &mut sink).unwrap().unwrap();
        assert_eq!(answer, "Two sources agree.");
        assert_eq!(usage.web_searches, 2);
    }

    #[test]
    fn a_server_side_count_still_wins_when_the_cli_reports_one() {
        let mut acc = Accumulator::new("opus");
        let mut sink = |_: &str| {};
        let done = r#"{"type":"result","is_error":false,"result":"ok","duration_ms":1,
            "usage":{"input_tokens":1,"output_tokens":1,"server_tool_use":{"web_search_requests":3}}}"#;
        let (_, usage) = acc.push(done, &mut sink).unwrap().unwrap();
        assert_eq!(usage.web_searches, 3);
    }

    #[test]
    fn an_error_result_is_a_failure_with_its_detail() {
        assert_eq!(
            parse_line(
                r#"{"type":"result","is_error":true,"result":"Not logged in"}"#,
                "opus"
            ),
            StreamLine::Failed("Claude CLI error: Not logged in".into())
        );
        assert_eq!(
            parse_line(r#"{"type":"result","is_error":true}"#, "opus"),
            StreamLine::Failed("Claude CLI error: no detail".into())
        );
    }

    fn fold<'a>(
        lines: impl IntoIterator<Item = &'a str>,
        on_text: &mut dyn FnMut(&str),
    ) -> Result<(String, ChatUsage), String> {
        let mut acc = Accumulator::new("opus");
        for line in lines {
            if let Some(done) = acc.push(line, on_text) {
                return done;
            }
        }
        Err(acc.finish())
    }

    #[test]
    fn folding_streams_each_chunk_in_order_and_ends_on_the_result() {
        let mut seen = Vec::new();
        let (answer, usage) = fold(
            [THINKING, DELTA, DELTA, RESULT, "ignored after result"],
            &mut |t| seen.push(t.to_string()),
        )
        .unwrap();
        assert_eq!(seen, vec!["Take ", "Take "]);
        assert_eq!(answer, "Take Chris Olave.");
        assert_eq!(usage.cost_usd, Some(0.31));
    }

    #[test]
    fn a_stream_cut_off_before_the_result_keeps_what_was_written() {
        let err = fold([DELTA], &mut |_| {}).unwrap_err();
        assert!(err.contains("stopped before finishing"), "{err}");
        let err = fold([THINKING], &mut |_| {}).unwrap_err();
        assert!(err.contains("empty answer"), "{err}");
    }

    #[test]
    fn prose_instead_of_json_is_reported_with_its_head() {
        let err = fold(["Take Olave.", "and more"], &mut |_| {}).unwrap_err();
        assert!(
            err.starts_with("unexpected Claude CLI output: Take Olave."),
            "{err}"
        );
    }

    #[test]
    fn an_empty_result_falls_back_to_the_streamed_text() {
        let (answer, _) = fold(
            [DELTA, r#"{"type":"result","is_error":false,"result":""}"#],
            &mut |_| {},
        )
        .unwrap();
        assert_eq!(answer, "Take");
        let err = fold(
            [r#"{"type":"result","is_error":false,"result":""}"#],
            &mut |_| {},
        )
        .unwrap_err();
        assert!(err.contains("empty answer"), "{err}");
    }
}
