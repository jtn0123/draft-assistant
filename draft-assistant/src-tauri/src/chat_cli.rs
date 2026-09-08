//! Ask Claude through the Claude Code CLI instead of the API.
//!
//! `claude -p` runs one headless turn and prints a JSON result, authenticated
//! with whatever the user logged the CLI into — a Claude subscription, most
//! likely — so no API key has to be pasted into the app. The board goes in as
//! the system prompt, exactly as it does over the API; the CLI's own tools are
//! switched off so it can only read what it is given.

use crate::chat::{ChatMessage, ChatModel, ChatReply, Effort};
use crate::chat_client::CancelSignal;
use crate::chat_copy::GUIDANCE;
use serde::Deserialize;
use std::path::{Path, PathBuf};
use std::process::Stdio;
use std::sync::Arc;
use std::time::Duration;
use tokio::io::{AsyncBufReadExt, AsyncWriteExt, BufReader};
use tokio::process::Command;

/// Reading the CLI's `stream-json` lines: the text deltas and the result.
#[path = "chat_cli_stream.rs"]
mod stream;

use stream::{is_result_line, text_delta};

/// A subscription-backed answer can take a while at high effort; give it room.
/// Public within the crate because the shared thread's deadline has to outwait
/// this route as well as the API one.
pub(crate) const TIMEOUT: Duration = Duration::from_secs(240);

/// Where the CLI is found. A Tauri app launched from the Dock does not inherit
/// the shell's PATH, so the usual install locations are checked by hand
/// before falling back to whatever PATH the process did get.
pub fn find_cli() -> Option<PathBuf> {
    let mut candidates: Vec<PathBuf> = Vec::new();
    if let Some(home) = std::env::var_os("HOME") {
        candidates.push(Path::new(&home).join(".local/bin/claude"));
        candidates.push(Path::new(&home).join(".claude/local/claude"));
    }
    candidates.push(PathBuf::from("/opt/homebrew/bin/claude"));
    candidates.push(PathBuf::from("/usr/local/bin/claude"));
    if let Some(found) = candidates.iter().find(|p| p.is_file()) {
        return Some(found.clone());
    }
    // Only now fall back to PATH, and refuse any entry in a directory the
    // whole machine can write to — that is how a planted `claude` would get
    // executed with this app's privileges.
    let path = std::env::var_os("PATH")?;
    std::env::split_paths(&path)
        .filter(|dir| !is_world_writable(dir))
        .map(|dir| dir.join("claude"))
        .find(|p| p.is_file())
}

/// True when anyone on the machine can write to `dir`. The sticky bit (as on
/// `/tmp`) does not make it safe: a file there is still someone else's to
/// create first.
#[cfg(unix)]
fn is_world_writable(dir: &Path) -> bool {
    use std::os::unix::fs::MetadataExt;
    std::fs::metadata(dir)
        .map(|meta| meta.mode() & 0o002 != 0)
        .unwrap_or(false)
}

#[cfg(not(unix))]
fn is_world_writable(_dir: &Path) -> bool {
    false
}

/// One prompt from a whole thread. The CLI takes a single prompt per run, so
/// earlier turns are replayed as a transcript ahead of the live question.
fn render_prompt(messages: &[ChatMessage]) -> String {
    let (last, earlier) = match messages.split_last() {
        Some(split) => split,
        None => return String::new(),
    };
    if earlier.is_empty() {
        return last.content.clone();
    }
    let mut out = String::from("Earlier in this conversation:\n\n");
    for m in earlier {
        let who = if m.role == "assistant" { "You" } else { "User" };
        out.push_str(&format!("{who}: {}\n\n", m.content.trim()));
    }
    out.push_str("Now the user asks:\n\n");
    out.push_str(&last.content);
    out
}

#[derive(Deserialize, Default)]
struct CliUsage {
    #[serde(default)]
    input_tokens: u32,
    #[serde(default)]
    output_tokens: u32,
    #[serde(default)]
    cache_creation_input_tokens: u32,
    #[serde(default)]
    cache_read_input_tokens: u32,
}

#[derive(Deserialize)]
struct CliResult {
    #[serde(default)]
    result: String,
    #[serde(default)]
    is_error: bool,
    #[serde(default)]
    stop_reason: Option<String>,
    #[serde(default)]
    usage: CliUsage,
    /// Keyed by model id; the answering model is whichever matches the request
    /// (a small helper model also shows up here).
    #[serde(default, rename = "modelUsage")]
    model_usage: std::collections::HashMap<String, serde_json::Value>,
}

/// Turn the CLI's JSON into the same reply the API path produces.
fn parse_result(stdout: &str, requested: ChatModel) -> Result<ChatReply, String> {
    let parsed: CliResult = serde_json::from_str(stdout.trim())
        .map_err(|e| format!("unexpected Claude Code output: {e}"))?;
    if parsed.is_error {
        return Err(if parsed.result.trim().is_empty() {
            "Claude Code reported an error".to_string()
        } else {
            format!("Claude Code: {}", parsed.result.trim())
        });
    }
    let refused = parsed.stop_reason.as_deref() == Some("refusal");
    let truncated = parsed.stop_reason.as_deref() == Some("max_tokens");
    let model = parsed
        .model_usage
        .keys()
        .find(|k| k.contains(requested.id()))
        .cloned()
        .unwrap_or_else(|| requested.id().to_string());
    let text = if parsed.result.trim().is_empty() && refused {
        "Claude declined to answer that one.".to_string()
    } else {
        parsed.result
    };
    Ok(ChatReply {
        text: crate::chat::with_truncation_note(text, truncated),
        thinking: None,
        model,
        refused,
        truncated,
        cancelled: false,
        input_tokens: parsed.usage.input_tokens,
        output_tokens: parsed.usage.output_tokens,
        // API-equivalent estimates also include subscription cache usage.
        cache_creation_input_tokens: parsed.usage.cache_creation_input_tokens,
        cache_read_input_tokens: parsed.usage.cache_read_input_tokens,
        // Filled in by `commands_chat`, which is the layer that knows which
        // route ran and what it is allowed to cost.
        provider: String::new(),
        cost_usd: 0.0,
        screen_spend_usd: 0.0,
    })
}

/// Make "not logged in" read like what the user has to do about it.
fn friendly_failure(stderr: &str, code: Option<i32>) -> String {
    let text = stderr.trim();
    let lower = text.to_ascii_lowercase();
    if lower.contains("log in") || lower.contains("login") || lower.contains("not authenticated") {
        return "Claude Code is not signed in. Run `claude` in Terminal and log in once, then try again".to_string();
    }
    let tail: String = text
        .chars()
        .rev()
        .take(300)
        .collect::<Vec<_>>()
        .into_iter()
        .rev()
        .collect();
    match code {
        Some(c) if !tail.is_empty() => format!("Claude Code exited with status {c}: {tail}"),
        Some(c) => format!("Claude Code exited with status {c}"),
        None => "Claude Code was interrupted".to_string(),
    }
}

/// The answer the user stopped, on this route.
///
/// The CLI prints its JSON when the whole turn is done and not before, so
/// there is no half-written answer to hand back the way the API route has one:
/// what the panel needs from a stopped run is the flag, at once, rather than
/// four more minutes of "Thinking". Not an error, for the same reason the API
/// route's is not: the user asked for it.
fn cut_short(model: ChatModel) -> ChatReply {
    ChatReply {
        text: String::new(),
        thinking: None,
        model: model.id().to_string(),
        refused: false,
        truncated: false,
        cancelled: true,
        input_tokens: 0,
        output_tokens: 0,
        cache_creation_input_tokens: 0,
        cache_read_input_tokens: 0,
        provider: String::new(),
        cost_usd: 0.0,
        screen_spend_usd: 0.0,
    }
}

/// Ask through the CLI. `context` is the same serialized view the API path
/// puts in its system prompt. `cancel` is the signal the panel's Cancel button
/// pulls, the same one the API path races: without it here, Cancel was inert
/// on the route the app picks by default, and the panel sat on "Thinking" for
/// the whole of [`TIMEOUT`].
pub async fn ask(
    cli: &Path,
    model: ChatModel,
    effort: Effort,
    context: &str,
    messages: &[ChatMessage],
    cancel: Arc<CancelSignal>,
    progress: Option<crate::chat::OnProgress>,
) -> Result<ChatReply, String> {
    ask_within(
        cli, model, effort, context, messages, cancel, TIMEOUT, progress,
    )
    .await
}

/// The same, with the deadline passed in. Only [`ask`] and the process tests,
/// which cannot wait four minutes to watch one time out, call this.
#[allow(clippy::too_many_arguments)]
async fn ask_within(
    cli: &Path,
    model: ChatModel,
    effort: Effort,
    context: &str,
    messages: &[ChatMessage],
    cancel: Arc<CancelSignal>,
    timeout: Duration,
    progress: Option<crate::chat::OnProgress>,
) -> Result<ChatReply, String> {
    if messages.is_empty() {
        return Err("nothing to ask".into());
    }
    if cancel.is_cancelled() {
        return Ok(cut_short(model));
    }
    let system = format!("{GUIDANCE}\n\n{context}");
    let mut child = Command::new(cli)
        .arg("-p")
        // `stream-json` (which needs `--verbose`) is what lets the panel show
        // the answer as it is written rather than in one lump at the end;
        // `--include-partial-messages` is what puts the text deltas on it.
        .arg("--verbose")
        .arg("--output-format")
        .arg("stream-json")
        .arg("--include-partial-messages")
        // The user's own MCP servers have no business in a question about a
        // draft board, and they are not free: on this machine they put ~58k
        // tokens of tool schemas in front of every question, so most of what
        // the model read was tool documentation rather than the board.
        .arg("--strict-mcp-config")
        .arg("--mcp-config")
        .arg(r#"{"mcpServers":{}}"#)
        .arg("--model")
        .arg(model.id())
        .arg("--effort")
        .arg(effort.cli_effort())
        .arg("--system-prompt")
        .arg(&system)
        .arg("--tools")
        .arg("")
        .arg("--no-session-persistence")
        .stdin(Stdio::piped())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        // A child that outlives the wait below — one that timed out, or whose
        // stdin could not be written — would otherwise keep running with the
        // app's privileges until the machine was rebooted, one per question.
        .kill_on_drop(true)
        .spawn()
        .map_err(|e| format!("could not start Claude Code at {}: {e}", cli.display()))?;

    let prompt = render_prompt(messages);
    let stdout = child
        .stdout
        .take()
        .ok_or_else(|| "Claude Code gave the app no output to read".to_string())?;
    let stderr = child
        .stderr
        .take()
        .ok_or_else(|| "Claude Code gave the app no diagnostics pipe".to_string())?;
    // Writing and reading share one deadline. A CLI that never reads its
    // stdin blocks the write forever once the pipe buffer is full, and that
    // write used to sit outside the timeout entirely.
    let run = async move {
        // The prompt goes over stdin so a long transcript never hits ARG_MAX.
        if let Some(mut stdin) = child.stdin.take() {
            stdin
                .write_all(prompt.as_bytes())
                .await
                .map_err(|e| format!("could not send the prompt to Claude Code: {e}"))?;
        }
        // One JSON object per line: text deltas while the model writes, then
        // the `result` line the parser below reads. Handing the text over as
        // it lands is the whole point of `stream-json` — the panel showed a
        // finished wall of text before this.
        let mut lines = BufReader::new(stdout).lines();
        let mut written = String::new();
        let mut told_at: Option<std::time::Instant> = None;
        let mut result = String::new();
        // What the CLI printed before it printed anything that parses, so a
        // binary that is not the CLI at all can be quoted back rather than
        // reported as an answer that stopped early.
        let mut head = String::new();
        let mut saw_json = false;
        while let Some(line) = lines
            .next_line()
            .await
            .map_err(|e| format!("could not read Claude Code's answer: {e}"))?
        {
            if !saw_json {
                saw_json = line.trim_start().starts_with('{');
                if !saw_json && head.chars().count() < 160 {
                    head.push_str(line.trim());
                }
            }
            if let Some(piece) = text_delta(&line) {
                written.push_str(&piece);
                // Throttled the way the API route's is: the deltas are a few
                // characters each, and the panel re-renders on every one.
                if let Some(watcher) = &progress {
                    if told_at.is_none_or(|at| at.elapsed() >= crate::chat::PROGRESS_EVERY) {
                        told_at = Some(std::time::Instant::now());
                        watcher(&written);
                    }
                }
            } else if is_result_line(&line) {
                result = line;
            }
        }
        // The last words land in the same breath as the end of the answer and
        // the throttle above swallows them; the finished reply carries them,
        // but this is what the panel shows until it lands.
        if let Some(watcher) = &progress {
            if !written.is_empty() {
                watcher(&written);
            }
        }
        let rest = child
            .wait_with_output()
            .await
            .map_err(|e| format!("Claude Code failed: {e}"))?;
        Ok::<_, String>((result, head, saw_json, rest))
    };
    let run = async {
        let ((result, head, saw_json, mut output), stderr) =
            tokio::try_join!(run, stream::drain_stderr(stderr))?;
        output.stderr = stderr;
        Ok::<_, String>((result, head, saw_json, output))
    };
    let (result, head, saw_json, output) = tokio::select! {
        // Dropping the run drops the child, and `kill_on_drop` above turns
        // that into a kill: a stopped question does not leave the CLI
        // answering it for another four minutes with the app's privileges.
        () = cancel.cancelled() => return Ok(cut_short(model)),
        output = tokio::time::timeout(timeout, run) => output,
    }
    .map_err(|_| "Claude Code took too long to answer, try a lower effort".to_string())??;

    if !output.status.success() {
        let stderr = String::from_utf8_lossy(&output.stderr);
        // The CLI sometimes reports an error as JSON on stdout with a non-zero
        // exit; prefer that message when it parses.
        if let Err(message) = parse_result(&result, model) {
            if message.starts_with("Claude Code:") {
                return Err(message);
            }
        }
        return Err(friendly_failure(&stderr, output.status.code()));
    }
    if result.is_empty() {
        return Err(if !saw_json && !head.is_empty() {
            let head: String = head.chars().take(160).collect();
            format!("unexpected Claude Code output: {head}")
        } else {
            "Claude Code stopped before finishing, try again".to_string()
        });
    }
    parse_result(&result, model)
}

#[cfg(test)]
#[path = "chat_cli_process_tests.rs"]
mod process_tests;

#[cfg(test)]
mod tests {
    #[test]
    fn a_world_writable_directory_is_never_trusted_for_the_cli() {
        use super::is_world_writable;
        // /tmp is the canonical world-writable directory on macOS and Linux.
        assert!(is_world_writable(std::path::Path::new("/tmp")));
        assert!(!is_world_writable(std::path::Path::new("/usr/bin")));
        // A path that does not exist is not a reason to bail out.
        assert!(!is_world_writable(std::path::Path::new(
            "/nonexistent-dir-for-test"
        )));
    }

    use super::*;

    fn msg(role: &str, content: &str) -> ChatMessage {
        ChatMessage {
            role: role.into(),
            content: content.into(),
        }
    }

    #[test]
    fn a_single_question_is_sent_verbatim() {
        assert_eq!(
            render_prompt(&[msg("user", "Who's left at TE?")]),
            "Who's left at TE?"
        );
    }

    #[test]
    fn earlier_turns_are_replayed_as_a_transcript() {
        let prompt = render_prompt(&[
            msg("user", "Am I thin at RB?"),
            msg("assistant", "Yes, one starter."),
            msg("user", "Who fixes that?"),
        ]);
        assert!(prompt.starts_with("Earlier in this conversation:"));
        assert!(prompt.contains("User: Am I thin at RB?"));
        assert!(prompt.contains("You: Yes, one starter."));
        assert!(prompt.ends_with("Now the user asks:\n\nWho fixes that?"));
    }

    #[test]
    fn cli_json_maps_onto_the_api_reply_shape() {
        let json = r#"{"type":"result","subtype":"success","is_error":false,"result":"Take Bowers.",
            "stop_reason":"end_turn","usage":{"input_tokens":120,"output_tokens":9},
            "modelUsage":{"claude-haiku-4-5-20251001":{},"claude-opus-5":{}}}"#;
        let reply = parse_result(json, ChatModel::Opus5).unwrap();
        assert_eq!(reply.text, "Take Bowers.");
        assert_eq!(reply.model, "claude-opus-5");
        assert_eq!(reply.input_tokens, 120);
        assert!(!reply.refused);
    }

    #[test]
    fn an_error_result_becomes_a_readable_error() {
        let json = r#"{"is_error":true,"result":"Invalid model"}"#;
        assert_eq!(
            parse_result(json, ChatModel::Fable5).unwrap_err(),
            "Claude Code: Invalid model"
        );
    }

    #[test]
    fn a_login_failure_says_what_to_do() {
        let message = friendly_failure("Error: Please run /login first", Some(1));
        assert!(message.contains("Run `claude` in Terminal"));
    }

    #[test]
    fn off_effort_maps_to_the_lowest_cli_level() {
        assert_eq!(Effort::Off.cli_effort(), "low");
        assert_eq!(Effort::Max.cli_effort(), "max");
    }
}
