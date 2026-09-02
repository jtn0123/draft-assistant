//! Ask Claude through the Claude Code CLI instead of the API.
//!
//! `claude -p` runs one headless turn and prints a JSON result, authenticated
//! with whatever the user logged the CLI into — a Claude subscription, most
//! likely — so no API key has to be pasted into the app. The board goes in as
//! the system prompt, exactly as it does over the API; the CLI's own tools are
//! switched off so it can only read what it is given.

use crate::chat::{ChatMessage, ChatModel, ChatReply, Effort};
use crate::chat_copy::GUIDANCE;
use serde::Deserialize;
use std::path::{Path, PathBuf};
use std::process::Stdio;
use std::time::Duration;
use tokio::io::AsyncWriteExt;
use tokio::process::Command;

/// A subscription-backed answer can take a while at high effort; give it room.
const TIMEOUT: Duration = Duration::from_secs(240);

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
    let model = parsed
        .model_usage
        .keys()
        .find(|k| k.contains(requested.id()))
        .cloned()
        .unwrap_or_else(|| requested.id().to_string());
    Ok(ChatReply {
        text: if parsed.result.trim().is_empty() && refused {
            "Claude declined to answer that one.".to_string()
        } else {
            parsed.result
        },
        thinking: None,
        model,
        refused,
        input_tokens: parsed.usage.input_tokens,
        output_tokens: parsed.usage.output_tokens,
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
        return "Claude Code is not signed in — run `claude` in Terminal and log in once, then try again".to_string();
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

/// Ask through the CLI. `context` is the same serialized view the API path
/// puts in its system prompt.
pub async fn ask(
    cli: &Path,
    model: ChatModel,
    effort: Effort,
    context: &str,
    messages: &[ChatMessage],
) -> Result<ChatReply, String> {
    if messages.is_empty() {
        return Err("nothing to ask".into());
    }
    let system = format!("{GUIDANCE}\n\n{context}");
    let mut child = Command::new(cli)
        .arg("-p")
        .arg("--output-format")
        .arg("json")
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
        .spawn()
        .map_err(|e| format!("could not start Claude Code at {}: {e}", cli.display()))?;

    // The prompt goes over stdin so a long transcript never hits ARG_MAX.
    if let Some(mut stdin) = child.stdin.take() {
        stdin
            .write_all(render_prompt(messages).as_bytes())
            .await
            .map_err(|e| format!("could not send the prompt to Claude Code: {e}"))?;
    }

    let output = tokio::time::timeout(TIMEOUT, child.wait_with_output())
        .await
        .map_err(|_| "Claude Code took too long to answer — try a lower effort".to_string())?
        .map_err(|e| format!("Claude Code failed: {e}"))?;

    let stdout = String::from_utf8_lossy(&output.stdout);
    if !output.status.success() {
        let stderr = String::from_utf8_lossy(&output.stderr);
        // The CLI sometimes reports an error as JSON on stdout with a non-zero
        // exit; prefer that message when it parses.
        if let Err(message) = parse_result(&stdout, model) {
            if message.starts_with("Claude Code:") {
                return Err(message);
            }
        }
        return Err(friendly_failure(&stderr, output.status.code()));
    }
    parse_result(&stdout, model)
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
            msg("assistant", "Yes — one starter."),
            msg("user", "Who fixes that?"),
        ]);
        assert!(prompt.starts_with("Earlier in this conversation:"));
        assert!(prompt.contains("User: Am I thin at RB?"));
        assert!(prompt.contains("You: Yes — one starter."));
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
        assert!(message.contains("run `claude` in Terminal"));
    }

    #[test]
    fn off_effort_maps_to_the_lowest_cli_level() {
        assert_eq!(Effort::Off.cli_effort(), "low");
        assert_eq!(Effort::Max.cli_effort(), "max");
    }
}
