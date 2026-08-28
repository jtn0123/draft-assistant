//! Ask Claude about the live draft, by shelling out to the locally installed
//! `claude` CLI.
//!
//! The CLI is used rather than the Messages API because it is already
//! authenticated on this machine and needs no API key. The seam is deliberately
//! narrow — one `ask` taking the view and a question and returning text — so
//! swapping in a direct HTTP call later touches only this file.

use crate::view::DraftView;
use serde_json::Value;
use std::path::{Path, PathBuf};
use std::process::Stdio;
use std::time::Duration;
use tokio::io::AsyncWriteExt;
use tokio::process::Command;

/// The full `DraftView` is ~34k tokens, almost all of it deep bench players
/// nobody asks about. The top slice carries every player realistically in play
/// plus all the roster, tier, and recommendation context.
const AVAILABLE_LIMIT: usize = 40;

/// Generous: a cold call measured ~10s, but a hung process must not wedge the
/// panel for the rest of the draft.
const TIMEOUT: Duration = Duration::from_secs(120);

const SYSTEM_PROMPT: &str =
    "You are a fantasy football draft assistant embedded in a live draft app. \
The user sends the current draft state as JSON, then asks a question. \
Answer only from that state — never invent players, projections, or picks that are not in it. \
Key fields: `available` is the board sorted by value (`vorp` is value over replacement, \
`survival` is the probability the player is still there at the user's next pick, `tier` \
groups similar players); `my_roster` is the user's team; `recommendations` is the app's own \
suggestion. Be direct and brief — two or three sentences unless asked for more. The user is \
mid-draft and reading fast, so lead with the answer, then the reason.";

/// Tried in order when `claude` is not on `PATH`. A packaged .app gets a
/// minimal environment, so relying on `PATH` alone works under `tauri dev` and
/// then fails in the bundle.
const FALLBACK_PATHS: [&str; 3] = [
    "~/.local/bin/claude",
    "/opt/homebrew/bin/claude",
    "/usr/local/bin/claude",
];

fn validate_question(question: &str) -> Result<&str, String> {
    let trimmed = question.trim();
    if trimmed.is_empty() {
        return Err("Ask a question first".into());
    }
    Ok(trimmed)
}

/// Cap the board to what a chat answer actually needs, leaving every other
/// field intact.
fn trim_available(value: &mut Value, limit: usize) {
    if let Some(available) = value.get_mut("available").and_then(Value::as_array_mut) {
        available.truncate(limit);
    }
}

pub fn trim_state(view: &DraftView, limit: usize) -> Result<Value, String> {
    let mut value = serde_json::to_value(view).map_err(|e| format!("serialize state: {e}"))?;
    trim_available(&mut value, limit);
    Ok(value)
}

/// Resolve the CLI. `DRAFT_ASSISTANT_CLAUDE_BIN` overrides everything.
fn claude_binary() -> PathBuf {
    if let Some(override_path) = std::env::var_os("DRAFT_ASSISTANT_CLAUDE_BIN") {
        return PathBuf::from(override_path);
    }
    for candidate in FALLBACK_PATHS {
        let path = match candidate.strip_prefix("~/") {
            Some(rest) => match std::env::var_os("HOME") {
                Some(home) => PathBuf::from(home).join(rest),
                None => continue,
            },
            None => PathBuf::from(candidate),
        };
        if path.is_file() {
            return path;
        }
    }
    // Last resort: let the OS search PATH, and report clearly if that fails.
    PathBuf::from("claude")
}

pub fn build_prompt(state: &Value, question: &str) -> String {
    format!(
        "Current draft state:\n```json\n{state}\n```\n\nQuestion: {question}",
        state = serde_json::to_string(state).unwrap_or_else(|_| "{}".into()),
    )
}

/// Run the CLI once and return its stdout.
async fn run_claude(binary: &Path, prompt: &str) -> Result<String, String> {
    // `--restricted` drops the tools that run commands or code: this is a chat
    // panel, not a coding agent. `--bare` is deliberately NOT used — it forces
    // API-key auth and would break the CLI's existing subscription login.
    let mut child = Command::new(binary)
        .arg("--print")
        .arg("--restricted")
        .arg("--no-session-persistence")
        .arg("--model")
        .arg("opus")
        .arg("--append-system-prompt")
        .arg(SYSTEM_PROMPT)
        .stdin(Stdio::piped())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .spawn()
        .map_err(|e| {
            format!(
                "could not run the Claude CLI at {} ({e}). Install Claude Code, or set \
                 DRAFT_ASSISTANT_CLAUDE_BIN to its full path.",
                binary.display()
            )
        })?;

    // The prompt goes over stdin, not argv: it is ~23KB and would sit near the
    // platform argument-length limit.
    let mut stdin = child
        .stdin
        .take()
        .ok_or_else(|| "Claude CLI stdin unavailable".to_string())?;
    stdin
        .write_all(prompt.as_bytes())
        .await
        .map_err(|e| format!("send prompt: {e}"))?;
    stdin
        .shutdown()
        .await
        .map_err(|e| format!("close prompt: {e}"))?;
    drop(stdin);

    let output = match tokio::time::timeout(TIMEOUT, child.wait_with_output()).await {
        Ok(result) => result.map_err(|e| format!("Claude CLI failed: {e}"))?,
        Err(_) => {
            return Err(format!(
                "Claude did not answer within {}s — try again",
                TIMEOUT.as_secs()
            ))
        }
    };

    if !output.status.success() {
        let stderr = String::from_utf8_lossy(&output.stderr);
        let detail = stderr.trim();
        let detail = if detail.is_empty() {
            "no error output".to_string()
        } else {
            detail.lines().take(3).collect::<Vec<_>>().join(" ")
        };
        return Err(format!("Claude CLI error: {detail}"));
    }

    let answer = String::from_utf8_lossy(&output.stdout).trim().to_string();
    if answer.is_empty() {
        return Err("Claude returned an empty answer — try again".into());
    }
    Ok(answer)
}

/// Ask Claude a question about the current draft.
pub async fn ask(view: &DraftView, question: &str) -> Result<String, String> {
    let question = validate_question(question)?;
    let prompt = build_prompt(&trim_state(view, AVAILABLE_LIMIT)?, question);
    run_claude(&claude_binary(), &prompt).await
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    fn board(n: usize) -> Value {
        json!({
            "available": (0..n).map(|i| json!({"player_id": i.to_string()})).collect::<Vec<_>>(),
            "my_roster": {"slots": []},
            "recommendations": [{"mode": "balanced"}],
        })
    }

    /// A stub standing in for the CLI, so the spawn/stdin/stdout path is
    /// exercised without a network call.
    fn stub(label: &str, script: &str) -> PathBuf {
        let path = std::env::temp_dir().join(format!(
            "draft-assistant-stub-{label}-{}",
            std::process::id()
        ));
        std::fs::write(&path, script).unwrap();
        #[cfg(unix)]
        {
            use std::os::unix::fs::PermissionsExt;
            std::fs::set_permissions(&path, std::fs::Permissions::from_mode(0o755)).unwrap();
        }
        path
    }

    #[test]
    fn trim_caps_the_board_and_keeps_the_reasoning_context() {
        let mut value = board(100);
        trim_available(&mut value, AVAILABLE_LIMIT);

        assert_eq!(
            value["available"].as_array().unwrap().len(),
            AVAILABLE_LIMIT
        );
        // The fields the model actually reasons from must survive the trim.
        assert!(value.get("my_roster").is_some());
        assert!(value.get("recommendations").is_some());
    }

    #[test]
    fn trim_leaves_a_short_board_alone() {
        let mut value = board(5);
        trim_available(&mut value, AVAILABLE_LIMIT);
        assert_eq!(value["available"].as_array().unwrap().len(), 5);
    }

    #[test]
    fn blank_questions_are_rejected() {
        assert!(validate_question("   \n ").is_err());
        assert_eq!(validate_question("  who? ").unwrap(), "who?");
    }

    #[test]
    fn prompt_carries_both_the_state_and_the_question() {
        let prompt = build_prompt(&board(1), "Who should I take?");
        assert!(prompt.contains("Who should I take?"));
        assert!(prompt.contains("player_id"));
    }

    #[tokio::test]
    async fn a_missing_cli_explains_how_to_fix_it() {
        let err = run_claude(Path::new("/nonexistent/claude"), "hi")
            .await
            .unwrap_err();
        assert!(err.contains("DRAFT_ASSISTANT_CLAUDE_BIN"), "{err}");
    }

    #[tokio::test]
    async fn the_answer_is_read_from_stdout() {
        let path = stub(
            "ok",
            "#!/bin/sh\ncat >/dev/null\necho 'Take Chris Olave.'\n",
        );
        let answer = run_claude(&path, "prompt").await.unwrap();
        assert_eq!(answer, "Take Chris Olave.");
        std::fs::remove_file(path).unwrap();
    }

    #[tokio::test]
    async fn a_failing_cli_surfaces_its_stderr() {
        let path = stub(
            "fail",
            "#!/bin/sh\ncat >/dev/null\necho 'not logged in' >&2\nexit 1\n",
        );
        let err = run_claude(&path, "prompt").await.unwrap_err();
        assert!(err.contains("not logged in"), "{err}");
        std::fs::remove_file(path).unwrap();
    }

    #[tokio::test]
    async fn an_empty_answer_is_an_error_not_a_blank_bubble() {
        let path = stub("empty", "#!/bin/sh\ncat >/dev/null\nprintf ''\n");
        let err = run_claude(&path, "prompt").await.unwrap_err();
        assert!(err.contains("empty answer"), "{err}");
        std::fs::remove_file(path).unwrap();
    }
}
