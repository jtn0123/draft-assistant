//! Running the `claude` CLI: locating the binary, translating the panel's
//! options into flags, and reading the JSON result it prints.

use super::ChatOptions;
use serde::{Deserialize, Serialize};
use std::ffi::OsString;
use std::path::{Path, PathBuf};
use std::process::Stdio;
use std::time::Duration;
use tokio::io::AsyncWriteExt;
use tokio::process::Command;

/// Tried in order when `claude` is not on `PATH`. A packaged .app gets a
/// minimal environment, so relying on `PATH` alone works under `tauri dev` and
/// then fails in the bundle.
const FALLBACK_PATHS: [&str; 3] = [
    "~/.local/bin/claude",
    "/opt/homebrew/bin/claude",
    "/usr/local/bin/claude",
];

/// Aliases the CLI resolves to a current model. Anything else from the panel
/// is refused rather than passed through to argv.
const MODELS: [&str; 4] = ["opus", "sonnet", "fable", "haiku"];
const EFFORTS: [&str; 5] = ["low", "medium", "high", "xhigh", "max"];
const DEFAULT_MODEL: &str = "opus";

pub struct Request<'a> {
    pub prompt: &'a str,
    pub system_prompt: &'a str,
    pub options: &'a ChatOptions,
    pub timeout: Duration,
}

/// What one call cost, as the CLI reports it. `context_tokens` is everything
/// the model read (fresh + cached input) — the number that tells the user how
/// big the thread has grown.
#[derive(Debug, Clone, Default, Serialize, Deserialize)]
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

/// The model to run: the panel's choice, else `DRAFT_ASSISTANT_CLAUDE_MODEL`,
/// else opus.
pub fn model_for(options: &ChatOptions) -> Result<String, String> {
    let chosen = options
        .model
        .clone()
        .filter(|m| !m.trim().is_empty())
        .or_else(|| std::env::var("DRAFT_ASSISTANT_CLAUDE_MODEL").ok())
        .unwrap_or_else(|| DEFAULT_MODEL.into());
    let chosen = chosen.trim().to_ascii_lowercase();
    if MODELS.contains(&chosen.as_str()) {
        Ok(chosen)
    } else {
        Err(format!(
            "unknown model '{chosen}' — choose one of {}",
            MODELS.join(", ")
        ))
    }
}

fn effort_for(options: &ChatOptions) -> Result<Option<String>, String> {
    let Some(effort) = options.effort.as_deref().map(str::trim) else {
        return Ok(None);
    };
    if effort.is_empty() {
        return Ok(None);
    }
    let effort = effort.to_ascii_lowercase();
    if EFFORTS.contains(&effort.as_str()) {
        Ok(Some(effort))
    } else {
        Err(format!(
            "unknown effort '{effort}' — choose one of {}",
            EFFORTS.join(", ")
        ))
    }
}

/// Flags for one call. `--restricted` drops the tools that run commands or
/// code: this is a chat panel, not a coding agent. `--bare` is deliberately
/// NOT used — it forces API-key auth and would break the CLI's existing
/// subscription login. `--tools` names exactly what is left: nothing, or web
/// search when the panel asked for it.
pub fn args(options: &ChatOptions, system_prompt: &str) -> Result<Vec<OsString>, String> {
    let mut args: Vec<OsString> = vec![
        "--print".into(),
        "--restricted".into(),
        "--no-session-persistence".into(),
        "--output-format".into(),
        "json".into(),
        "--model".into(),
        model_for(options)?.into(),
        "--tools".into(),
        if options.web_search {
            "WebSearch".into()
        } else {
            "".into()
        },
    ];
    if let Some(effort) = effort_for(options)? {
        args.push("--effort".into());
        args.push(effort.into());
    }
    if options.fast {
        // Fast mode is a per-session opt-in. Whether it actually served is
        // reported back in `fast_mode_state`, so the panel can say why not.
        args.push("--settings".into());
        args.push(r#"{"fastMode":true}"#.into());
    }
    args.push("--append-system-prompt".into());
    args.push(system_prompt.into());
    Ok(args)
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

#[derive(Deserialize)]
struct CliResult {
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
    fast_mode_state: Option<String>,
    #[serde(default)]
    fast_mode_disabled_reason: Option<String>,
}

/// Read the single JSON object `--output-format json` prints.
pub fn parse_result(stdout: &str, model: &str) -> Result<(String, ChatUsage), String> {
    let trimmed = stdout.trim();
    if trimmed.is_empty() {
        return Err("Claude returned an empty answer — try again".into());
    }
    let parsed: CliResult = serde_json::from_str(trimmed).map_err(|e| {
        let head: String = trimmed.chars().take(160).collect();
        format!("unexpected Claude CLI output ({e}): {head}")
    })?;
    let answer = parsed.result.trim().to_string();
    if parsed.is_error {
        let detail = if answer.is_empty() {
            "no detail".to_string()
        } else {
            answer
        };
        return Err(format!("Claude CLI error: {detail}"));
    }
    if answer.is_empty() {
        return Err("Claude returned an empty answer — try again".into());
    }
    let u = parsed.usage;
    let usage = ChatUsage {
        model: model.to_string(),
        input_tokens: u.input_tokens,
        cache_read_tokens: u.cache_read_input_tokens,
        cache_write_tokens: u.cache_creation_input_tokens,
        output_tokens: u.output_tokens,
        context_tokens: u.input_tokens + u.cache_read_input_tokens + u.cache_creation_input_tokens,
        web_searches: u.server_tool_use.web_search_requests,
        duration_ms: parsed.duration_ms,
        cost_usd: parsed.total_cost_usd,
        fast_mode: parsed.fast_mode_state,
        fast_mode_reason: parsed.fast_mode_disabled_reason,
    };
    Ok((answer, usage))
}

pub async fn run(request: &Request<'_>) -> Result<(String, ChatUsage), String> {
    run_at(&claude_binary(), request).await
}

async fn run_at(binary: &Path, request: &Request<'_>) -> Result<(String, ChatUsage), String> {
    let model = model_for(request.options)?;
    let mut child = Command::new(binary)
        .args(args(request.options, request.system_prompt)?)
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

    // The prompt goes over stdin, not argv: with the whole board it is
    // ~40KB, well past the platform argument-length limit.
    let mut stdin = child
        .stdin
        .take()
        .ok_or_else(|| "Claude CLI stdin unavailable".to_string())?;
    stdin
        .write_all(request.prompt.as_bytes())
        .await
        .map_err(|e| format!("send prompt: {e}"))?;
    stdin
        .shutdown()
        .await
        .map_err(|e| format!("close prompt: {e}"))?;
    drop(stdin);

    let output = match tokio::time::timeout(request.timeout, child.wait_with_output()).await {
        Ok(result) => result.map_err(|e| format!("Claude CLI failed: {e}"))?,
        Err(_) => {
            return Err(format!(
                "Claude did not answer within {}s — try again",
                request.timeout.as_secs()
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

    parse_result(&String::from_utf8_lossy(&output.stdout), &model)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn opts(model: Option<&str>, effort: Option<&str>, fast: bool, web: bool) -> ChatOptions {
        ChatOptions {
            model: model.map(String::from),
            effort: effort.map(String::from),
            fast,
            web_search: web,
        }
    }

    fn request<'a>(prompt: &'a str, options: &'a ChatOptions) -> Request<'a> {
        Request {
            prompt,
            system_prompt: "sys",
            options,
            timeout: Duration::from_secs(10),
        }
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

    fn strings(args: &[OsString]) -> Vec<String> {
        args.iter()
            .map(|a| a.to_string_lossy().into_owned())
            .collect()
    }

    #[test]
    fn defaults_are_opus_no_tools_no_effort_flag() {
        let a = strings(&args(&ChatOptions::default(), "sys").unwrap());
        let joined = a.join(" ");
        assert!(joined.contains("--model opus"), "{joined}");
        assert!(joined.contains("--output-format json"), "{joined}");
        assert!(joined.contains("--restricted"), "{joined}");
        assert!(!joined.contains("--effort"), "{joined}");
        assert!(!joined.contains("fastMode"), "{joined}");
        let tools = a.iter().position(|x| x == "--tools").unwrap();
        assert_eq!(a[tools + 1], "");
        assert_eq!(a.last().unwrap(), "sys");
    }

    #[test]
    fn every_selector_reaches_argv() {
        let a = strings(&args(&opts(Some("Sonnet"), Some("xhigh"), true, true), "sys").unwrap());
        let joined = a.join(" ");
        assert!(joined.contains("--model sonnet"), "{joined}");
        assert!(joined.contains("--effort xhigh"), "{joined}");
        assert!(joined.contains("--tools WebSearch"), "{joined}");
        assert!(
            joined.contains(r#"--settings {"fastMode":true}"#),
            "{joined}"
        );
    }

    #[test]
    fn unknown_models_and_efforts_are_refused_not_passed_through() {
        let err = args(&opts(Some("gpt-9"), None, false, false), "sys").unwrap_err();
        assert!(err.contains("unknown model"), "{err}");
        let err = args(&opts(None, Some("ultra"), false, false), "sys").unwrap_err();
        assert!(err.contains("unknown effort"), "{err}");
        // Blank strings mean "default", not an error.
        assert!(args(&opts(Some(""), Some(" "), false, false), "sys").is_ok());
    }

    #[test]
    fn the_json_result_yields_the_answer_and_usage() {
        let json = r#"{"type":"result","subtype":"success","is_error":false,"duration_ms":9120,
            "result":"  Take Chris Olave.  ","total_cost_usd":0.31,
            "usage":{"input_tokens":12000,"cache_creation_input_tokens":3000,"cache_read_input_tokens":15000,
                     "output_tokens":80,"server_tool_use":{"web_search_requests":2}},
            "fast_mode_state":"off","fast_mode_disabled_reason":"extra_usage_disabled"}"#;
        let (answer, usage) = parse_result(json, "opus").unwrap();
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
    fn an_error_result_or_garbage_is_an_error_not_a_bubble() {
        let err =
            parse_result(r#"{"is_error":true,"result":"Not logged in"}"#, "opus").unwrap_err();
        assert!(err.contains("Not logged in"), "{err}");
        let err = parse_result("Take Olave.", "opus").unwrap_err();
        assert!(err.contains("unexpected Claude CLI output"), "{err}");
        assert!(err.contains("Take Olave."), "{err}");
        let err = parse_result(r#"{"is_error":false,"result":""}"#, "opus").unwrap_err();
        assert!(err.contains("empty answer"), "{err}");
    }

    #[tokio::test]
    async fn a_missing_cli_explains_how_to_fix_it() {
        let options = ChatOptions::default();
        let err = run_at(Path::new("/nonexistent/claude"), &request("hi", &options))
            .await
            .unwrap_err();
        assert!(err.contains("DRAFT_ASSISTANT_CLAUDE_BIN"), "{err}");
    }

    #[tokio::test]
    async fn the_answer_is_read_from_stdout_and_the_flags_are_passed() {
        // The stub echoes its own argv as the answer, so the test can see
        // exactly what the CLI would have received.
        let path = stub(
            "ok",
            "#!/bin/sh\ncat >/dev/null\nprintf '{\"is_error\":false,\"result\":\"args: %s\",\"usage\":{\"input_tokens\":7}}' \"$*\"\n",
        );
        let options = opts(Some("sonnet"), Some("low"), false, true);
        let (answer, usage) = run_at(&path, &request("prompt", &options)).await.unwrap();
        assert!(answer.contains("--model sonnet"), "{answer}");
        assert!(answer.contains("--tools WebSearch"), "{answer}");
        assert!(answer.contains("--effort low"), "{answer}");
        assert_eq!(usage.context_tokens, 7);
        assert_eq!(usage.model, "sonnet");
        std::fs::remove_file(path).unwrap();
    }

    #[tokio::test]
    async fn a_failing_cli_surfaces_its_stderr() {
        let path = stub(
            "fail",
            "#!/bin/sh\ncat >/dev/null\necho 'not logged in' >&2\nexit 1\n",
        );
        let options = ChatOptions::default();
        let err = run_at(&path, &request("prompt", &options))
            .await
            .unwrap_err();
        assert!(err.contains("not logged in"), "{err}");
        std::fs::remove_file(path).unwrap();
    }

    #[tokio::test]
    async fn a_hung_cli_is_cut_off_at_the_timeout() {
        let path = stub("hang", "#!/bin/sh\ncat >/dev/null\nsleep 5\n");
        let options = ChatOptions::default();
        let req = Request {
            prompt: "prompt",
            system_prompt: "sys",
            options: &options,
            timeout: Duration::from_millis(200),
        };
        let err = run_at(&path, &req).await.unwrap_err();
        assert!(err.contains("did not answer"), "{err}");
        std::fs::remove_file(path).unwrap();
    }
}
