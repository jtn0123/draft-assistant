//! Subscription-backed OpenAI chat through `codex exec`, isolated from projects.
//! Auth stays in Codex; no tokens are read or copied by this app. The model gets
//! only the board and conversation, with tools, plugins and user config off.

use crate::chat::{ChatMessage, ChatModel, ChatReply, Effort};
use crate::chat_client::CancelSignal;
use serde_json::Value;
use std::path::{Path, PathBuf};
use std::process::Stdio;
use std::sync::{
    atomic::{AtomicU64, Ordering},
    Arc,
};
use std::time::Duration;
use tokio::io::{AsyncReadExt, AsyncWriteExt};
use tokio::process::Command;

pub(crate) const TIMEOUT: Duration = Duration::from_secs(240);
const MAX_OUTPUT: u64 = 2_000_000;
const DISABLED: &[&str] = &[
    "shell_tool",
    "unified_exec",
    "apply_patch_freeform",
    "apps",
    "plugins",
    "hooks",
    "multi_agent",
    "browser_use",
    "computer_use",
    "view_image",
    "image_generation",
    "memories",
    "skill_search",
    "js_repl",
    "code_mode",
    "goals",
];

pub fn find_cli() -> Option<PathBuf> {
    let mut paths = vec![
        PathBuf::from("/opt/homebrew/bin/codex"),
        PathBuf::from("/usr/local/bin/codex"),
    ];
    if let Some(home) = std::env::var_os("HOME") {
        paths.push(Path::new(&home).join(".local/bin/codex"));
        paths.push(Path::new(&home).join(".npm-global/bin/codex"));
    }
    if let Some(path) = std::env::var_os("PATH") {
        paths.extend(
            std::env::split_paths(&path)
                .filter(|p| safe_directory(p))
                .map(|p| p.join("codex")),
        );
    }
    paths.into_iter().find(|p| p.is_file())
}

fn safe_directory(path: &Path) -> bool {
    #[cfg(unix)]
    {
        use std::os::unix::fs::MetadataExt;
        path.is_absolute() && std::fs::metadata(path).is_ok_and(|m| m.mode() & 0o002 == 0)
    }
    #[cfg(not(unix))]
    {
        path.is_absolute()
    }
}

struct Scratch(PathBuf);
impl Scratch {
    fn new() -> Result<Self, String> {
        static NEXT: AtomicU64 = AtomicU64::new(0);
        let stamp = std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .unwrap_or_default()
            .as_nanos();
        let path = std::env::temp_dir().join(format!(
            "draft-ai-{}-{stamp}-{}",
            std::process::id(),
            NEXT.fetch_add(1, Ordering::Relaxed)
        ));
        let mut builder = std::fs::DirBuilder::new();
        #[cfg(unix)]
        {
            use std::os::unix::fs::DirBuilderExt;
            builder.mode(0o700);
        }
        builder
            .create(&path)
            .map_err(|e| format!("Cannot prepare AI chat: {e}"))?;
        Ok(Self(path))
    }
}
impl Drop for Scratch {
    fn drop(&mut self) {
        std::fs::remove_dir_all(&self.0).ok();
    }
}

fn blank(model: ChatModel) -> ChatReply {
    ChatReply {
        text: String::new(),
        thinking: None,
        model: model.id().to_string(),
        refused: false,
        truncated: false,
        cancelled: false,
        input_tokens: 0,
        output_tokens: 0,
        cache_creation_input_tokens: 0,
        cache_read_input_tokens: 0,
        provider: String::new(),
        cost_usd: 0.0,
        screen_spend_usd: 0.0,
    }
}

fn number(v: &Value, key: &str) -> u32 {
    v[key].as_u64().unwrap_or(0).min(u64::from(u32::MAX)) as u32
}

/// A completed turn is required: partial JSON or an early process exit is not
/// a successful answer. A failed turn must not hide behind an earlier message.
fn parse_output(stdout: &[u8], model: ChatModel) -> Result<ChatReply, String> {
    let stdout = std::str::from_utf8(stdout).map_err(|_| "Codex returned invalid text")?;
    let mut reply = blank(model);
    let mut complete = false;
    for line in stdout.lines().filter(|s| !s.trim().is_empty()) {
        let event: Value =
            serde_json::from_str(line).map_err(|_| "Codex returned malformed JSON")?;
        match event["type"].as_str().unwrap_or("") {
            "item.completed" if event["item"]["type"] == "agent_message" => {
                reply.text = event["item"]["text"].as_str().unwrap_or("").to_string();
            }
            "turn.completed" => {
                let usage = &event["usage"];
                reply.cache_read_input_tokens = number(usage, "cached_input_tokens");
                reply.cache_creation_input_tokens = number(usage, "cache_write_input_tokens");
                // Codex input includes cached tokens; Anthropic's wire does not.
                reply.input_tokens = number(usage, "input_tokens")
                    .saturating_sub(reply.cache_read_input_tokens)
                    .saturating_sub(reply.cache_creation_input_tokens);
                reply.output_tokens = number(usage, "output_tokens");
                complete = true;
            }
            "turn.failed" => {
                let message = event["error"]["message"]
                    .as_str()
                    .unwrap_or("the turn failed");
                return Err(format!(
                    "Codex: {}",
                    message.chars().take(500).collect::<String>()
                ));
            }
            _ => {}
        }
    }
    if !complete || reply.text.trim().is_empty() {
        return Err("Codex did not finish an answer. Check `codex login` and access to the selected model, then retry.".into());
    }
    Ok(reply)
}

fn command(cli: &Path, cwd: &Path, model: ChatModel, effort: Effort) -> Command {
    let mut cmd = Command::new(cli);
    cmd.args([
        "exec",
        "--ignore-user-config",
        "--ephemeral",
        "--skip-git-repo-check",
        "--sandbox",
        "read-only",
        "--json",
        "--color",
        "never",
        "--cd",
    ])
    .arg(cwd)
    .arg("--model")
    .arg(model.id())
    .args([
        "-c",
        "approval_policy=\"never\"",
        "-c",
        "web_search=\"disabled\"",
        "-c",
        "project_doc_max_bytes=0",
        "-c",
        "features.skip_host_skill_discovery=true",
    ]);
    let effort = match effort {
        Effort::Off if model == ChatModel::Sol => "none",
        Effort::Off => "low",
        other => other.cli_effort(),
    };
    cmd.arg("-c")
        .arg(format!("model_reasoning_effort=\"{effort}\""));
    cmd.arg("-c").arg(format!(
        "developer_instructions={}",
        serde_json::to_string(crate::chat_copy::GUIDANCE).expect("string")
    ));
    for feature in DISABLED {
        cmd.arg("-c").arg(format!("features.{feature}=false"));
    }
    cmd.arg("-")
        .current_dir(cwd)
        .stdin(Stdio::piped())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .kill_on_drop(true);
    cmd
}

pub async fn ask(
    cli: &Path,
    model: ChatModel,
    effort: Effort,
    context: &str,
    messages: &[ChatMessage],
    cancel: Arc<CancelSignal>,
) -> Result<ChatReply, String> {
    ask_within(cli, model, effort, context, messages, cancel, TIMEOUT).await
}

async fn ask_within(
    cli: &Path,
    model: ChatModel,
    effort: Effort,
    context: &str,
    messages: &[ChatMessage],
    cancel: Arc<CancelSignal>,
    timeout: Duration,
) -> Result<ChatReply, String> {
    if !model.is_openai() {
        return Err("Choose an OpenAI model for the Codex route".into());
    }
    if messages.is_empty() {
        return Err("nothing to ask".into());
    }
    if cancel.is_cancelled() {
        let mut reply = blank(model);
        reply.cancelled = true;
        return Ok(reply);
    }
    let scratch = Scratch::new()?;
    let prompt =
        serde_json::json!({"board_context": context, "conversation": messages}).to_string();
    let mut child = command(cli, &scratch.0, model, effort)
        .spawn()
        .map_err(|e| {
            format!("Cannot start Codex: {e}. Install Codex and run `codex login` once.")
        })?;
    let mut stdin = child.stdin.take().ok_or("Codex stdin unavailable")?;
    let stdout = child.stdout.take().ok_or("Codex stdout unavailable")?;
    let stderr = child.stderr.take().ok_or("Codex stderr unavailable")?;
    let run = async {
        let read = async {
            let mut out = Vec::new();
            stdout
                .take(MAX_OUTPUT + 1)
                .read_to_end(&mut out)
                .await
                .map_err(|e| e.to_string())?;
            if out.len() as u64 > MAX_OUTPUT {
                return Err("Codex output was too large".to_string());
            }
            Ok(out)
        };
        let errors = async {
            let mut err = Vec::new();
            stderr
                .take(MAX_OUTPUT + 1)
                .read_to_end(&mut err)
                .await
                .map_err(|e| e.to_string())?;
            if err.len() as u64 > MAX_OUTPUT {
                return Err("Codex diagnostics were too large".to_string());
            }
            Ok(err)
        };
        let write = async {
            stdin
                .write_all(prompt.as_bytes())
                .await
                .map_err(|e| e.to_string())?;
            drop(stdin);
            Ok::<_, String>(())
        };
        let status = async { child.wait().await.map_err(|e| e.to_string()) };
        let (out, _err, (), status) = tokio::try_join!(read, errors, write, status)?;
        // Prefer the structured provider failure when available, never print
        // raw stderr, which can contain local paths or authentication details.
        let parsed = parse_output(&out, model);
        if !status.success() && parsed.is_ok() {
            return Err(
                "Codex exited before completing successfully. Check sign-in and model access."
                    .into(),
            );
        }
        parsed
    };
    let result = tokio::select! {
        _ = cancel.cancelled() => { let mut reply = blank(model); reply.cancelled = true; Ok(reply) },
        result = tokio::time::timeout(timeout, run) => result.unwrap_or_else(|_| Err("Codex took too long. Try a lower effort or retry.".into())),
    };
    // Also reap on cancel/timeout/output overflow; no background assistant is
    // left running after the panel stops waiting.
    if child.try_wait().ok().flatten().is_none() {
        child.kill().await.ok();
    }
    result
}

#[cfg(test)]
#[path = "chat_codex_tests.rs"]
mod tests;
