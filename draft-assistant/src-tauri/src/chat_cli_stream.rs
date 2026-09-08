//! Reading what `claude --output-format stream-json` prints.
//!
//! One JSON object per line. The answer arrives as `stream_event` /
//! `content_block_delta` / `text_delta` pieces while the model writes, and the
//! last line is the `result` object — the same shape `--output-format json`
//! used to print on its own, which is why the reply parser is unchanged.
//! Thinking deltas, tool traffic and status lines are nothing the panel shows.

use serde::Deserialize;

/// One line, in the two shapes this reads: a piece of the answer as it is
/// written, and the final result.
#[derive(Deserialize, Default)]
struct StreamLine {
    #[serde(default, rename = "type")]
    kind: String,
    #[serde(default)]
    event: StreamEvent,
}

#[derive(Deserialize, Default)]
struct StreamEvent {
    #[serde(default, rename = "type")]
    kind: String,
    #[serde(default)]
    delta: StreamDelta,
}

#[derive(Deserialize, Default)]
struct StreamDelta {
    #[serde(default, rename = "type")]
    kind: String,
    #[serde(default)]
    text: String,
}

/// The piece of the answer this line carries, or `None` for the many lines
/// that are status, thinking, tool traffic, or not JSON at all — the CLI puts
/// its own warnings on stdout now and then, and one must not end a stream that
/// is otherwise fine.
pub(super) fn text_delta(line: &str) -> Option<String> {
    let parsed: StreamLine = serde_json::from_str(line.trim()).ok()?;
    if parsed.kind != "stream_event" || parsed.event.kind != "content_block_delta" {
        return None;
    }
    (parsed.event.delta.kind == "text_delta").then_some(parsed.event.delta.text)
}

/// True when this line is the final `result` object — the one `parse_result`
/// reads, and the same shape `--output-format json` used to print on its own.
pub(super) fn is_result_line(line: &str) -> bool {
    serde_json::from_str::<serde_json::Value>(line.trim())
        .ok()
        .and_then(|v| v.get("type").and_then(|t| t.as_str().map(str::to_string)))
        .is_some_and(|kind| kind == "result")
}

/// Drain all stderr concurrently with stdout, retaining a bounded diagnostic.
/// Stopping after the retained prefix would leave the child blocked on its pipe.
pub(super) async fn drain_stderr(
    mut stderr: tokio::process::ChildStderr,
) -> Result<Vec<u8>, String> {
    use tokio::io::AsyncReadExt;
    const LIMIT: usize = 64 * 1024;
    let mut kept = Vec::new();
    let mut chunk = [0u8; 8192];
    loop {
        let count = stderr
            .read(&mut chunk)
            .await
            .map_err(|e| format!("could not read Claude Code's diagnostics: {e}"))?;
        if count == 0 {
            return Ok(kept);
        }
        let retain = count.min(LIMIT.saturating_sub(kept.len()));
        kept.extend_from_slice(&chunk[..retain]);
    }
}
