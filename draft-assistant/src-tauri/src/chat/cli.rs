//! Running the `claude` CLI: locating the binary, translating the panel's
//! options into flags, and streaming its answer back as it is written.

use super::stream::{self, ChatUsage};
use super::ChatOptions;
use std::ffi::OsString;
use std::path::{Path, PathBuf};
use std::process::Stdio;
use std::time::Duration;
use tokio::io::{AsyncBufReadExt, AsyncReadExt, AsyncWriteExt, BufReader};
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

/// Flags for one call.
///
/// `--restricted` drops the tools that run commands or code: this is a chat
/// panel, not a coding agent. `--tools` names exactly what is left: nothing,
/// or web search when the panel asked for it. `--strict-mcp-config` with an
/// empty server list keeps the user's own MCP servers out of the call — on
/// this machine they added ~16k tokens of tool schemas to every question, so
/// the board was a third of what the model read. `--bare` is deliberately
/// NOT used — it forces API-key auth and would break the CLI's existing
/// subscription login. `stream-json` (which needs `--verbose`) is what lets
/// the panel show the answer as it is written.
pub fn args(options: &ChatOptions, system_prompt: &str) -> Result<Vec<OsString>, String> {
    let mut args: Vec<OsString> = vec![
        "--print".into(),
        "--restricted".into(),
        "--no-session-persistence".into(),
        "--strict-mcp-config".into(),
        "--mcp-config".into(),
        r#"{"mcpServers":{}}"#.into(),
        "--verbose".into(),
        "--output-format".into(),
        "stream-json".into(),
        "--include-partial-messages".into(),
        "--model".into(),
        model_for(options)?.into(),
        "--tools".into(),
        if options.web_search {
            "WebSearch".into()
        } else {
            "".into()
        },
    ];
    if options.web_search {
        // `--tools` offers the tool; it does not permit it. Without this the
        // model asks for approval, nobody is there to give it under
        // `--print`, and the answer comes back "I don't have permission to
        // use web search" — which read as web search simply not working.
        args.push("--allowedTools".into());
        args.push("WebSearch".into());
    }
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

/// Run the CLI, handing each piece of the answer to `on_text` as it arrives,
/// and return the whole answer with its usage.
pub async fn run(
    request: &Request<'_>,
    on_text: &mut (dyn FnMut(&str) + Send),
) -> Result<(String, ChatUsage), String> {
    run_at(&claude_binary(), request, on_text).await
}

async fn run_at(
    binary: &Path,
    request: &Request<'_>,
    on_text: &mut (dyn FnMut(&str) + Send),
) -> Result<(String, ChatUsage), String> {
    let model = model_for(request.options)?;
    let mut child = Command::new(binary)
        .args(args(request.options, request.system_prompt)?)
        // A neutral directory: the CLI reads CLAUDE.md files from wherever it
        // is started, and the app's working directory is whatever launched it.
        .current_dir(std::env::temp_dir())
        .stdin(Stdio::piped())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .kill_on_drop(true)
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

    let stdout = child
        .stdout
        .take()
        .ok_or_else(|| "Claude CLI stdout unavailable".to_string())?;
    // Drain stderr on its own task so a chatty CLI cannot fill the pipe and
    // block itself while stdout is being read line by line.
    let mut stderr = child
        .stderr
        .take()
        .ok_or_else(|| "Claude CLI stderr unavailable".to_string())?;
    let stderr_task = tokio::spawn(async move {
        let mut buf = String::new();
        let _ = stderr.read_to_string(&mut buf).await;
        buf
    });

    let outcome =
        match tokio::time::timeout(request.timeout, read_stream(stdout, &model, on_text)).await {
            Ok(outcome) => outcome,
            Err(_) => {
                let _ = child.kill().await;
                return Err(format!(
                    "Claude did not answer within {}s — try again",
                    request.timeout.as_secs()
                ));
            }
        };
    let status = child
        .wait()
        .await
        .map_err(|e| format!("Claude CLI failed: {e}"))?;
    let stderr = stderr_task.await.unwrap_or_default();

    match outcome {
        Ok(done) => Ok(done),
        Err(err) if status.success() => Err(err),
        Err(_) => {
            let detail = stderr.trim();
            let detail = if detail.is_empty() {
                "no error output".to_string()
            } else {
                detail.lines().take(3).collect::<Vec<_>>().join(" ")
            };
            Err(format!("Claude CLI error: {detail}"))
        }
    }
}

/// Read stdout line by line until the `result` line (or EOF), streaming text.
async fn read_stream(
    stdout: tokio::process::ChildStdout,
    model: &str,
    on_text: &mut (dyn FnMut(&str) + Send),
) -> Result<(String, ChatUsage), String> {
    let mut lines = BufReader::new(stdout).lines();
    let mut acc = stream::Accumulator::new(model);
    while let Some(line) = lines
        .next_line()
        .await
        .map_err(|e| format!("read Claude CLI output: {e}"))?
    {
        if let Some(done) = acc.push(&line, on_text) {
            return done;
        }
    }
    Err(acc.finish())
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

    #[test]
    fn web_search_is_permitted_as_well_as_offered() {
        // `--tools` alone left the model asking for approval that nothing in
        // `--print` mode can give, and the answer came back saying it had no
        // permission to search.
        let on = args(&opts(None, None, false, true), "sys").unwrap();
        let flat: Vec<String> = on.iter().map(|a| a.to_string_lossy().into()).collect();
        let tools = flat.iter().position(|a| a == "--tools").unwrap();
        assert_eq!(flat[tools + 1], "WebSearch");
        let allowed = flat
            .iter()
            .position(|a| a == "--allowedTools")
            .expect("the tool has to be permitted, not just offered");
        assert_eq!(flat[allowed + 1], "WebSearch");

        // Off, neither flag names it and nothing is permitted.
        let off = args(&opts(None, None, false, false), "sys").unwrap();
        let flat: Vec<String> = off.iter().map(|a| a.to_string_lossy().into()).collect();
        assert!(!flat.iter().any(|a| a == "--allowedTools"), "{flat:?}");
        let tools = flat.iter().position(|a| a == "--tools").unwrap();
        assert_eq!(flat[tools + 1], "");
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

    async fn run_collecting(
        path: &Path,
        req: &Request<'_>,
    ) -> (Result<(String, ChatUsage), String>, Vec<String>) {
        let mut seen = Vec::new();
        let result = run_at(path, req, &mut |t| seen.push(t.to_string())).await;
        (result, seen)
    }

    #[test]
    fn defaults_are_opus_streaming_no_tools_no_mcp_no_effort_flag() {
        let a = strings(&args(&ChatOptions::default(), "sys").unwrap());
        let joined = a.join(" ");
        assert!(joined.contains("--model opus"), "{joined}");
        assert!(joined.contains("--output-format stream-json"), "{joined}");
        assert!(joined.contains("--verbose"), "{joined}");
        assert!(joined.contains("--include-partial-messages"), "{joined}");
        assert!(joined.contains("--restricted"), "{joined}");
        assert!(
            joined.contains(r#"--strict-mcp-config --mcp-config {"mcpServers":{}}"#),
            "{joined}"
        );
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

    #[tokio::test]
    async fn a_missing_cli_explains_how_to_fix_it() {
        let options = ChatOptions::default();
        let (result, _) =
            run_collecting(Path::new("/nonexistent/claude"), &request("hi", &options)).await;
        let err = result.unwrap_err();
        assert!(err.contains("DRAFT_ASSISTANT_CLAUDE_BIN"), "{err}");
    }

    #[tokio::test]
    async fn the_answer_streams_from_stdout_and_the_flags_are_passed() {
        // The stub streams two chunks, then a result line that echoes its own
        // argv and working directory, so the test sees exactly what the CLI
        // would have received.
        let path = stub(
            "ok",
            concat!(
                "#!/bin/sh\ncat >/dev/null\n",
                "printf '{\"type\":\"stream_event\",\"event\":{\"type\":\"content_block_delta\",\"delta\":{\"type\":\"text_delta\",\"text\":\"Take \"}}}\\n'\n",
                "printf '{\"type\":\"stream_event\",\"event\":{\"type\":\"content_block_delta\",\"delta\":{\"type\":\"text_delta\",\"text\":\"Olave.\"}}}\\n'\n",
                // The argv holds JSON of its own; its quotes are dropped so the
                // echoed line is still one valid JSON object.
                "printf '{\"type\":\"result\",\"is_error\":false,\"result\":\"args: %s cwd: %s\",\"usage\":{\"input_tokens\":7}}\\n' \"$(printf '%s' \"$*\" | tr -d '\"')\" \"$PWD\"\n",
            ),
        );
        let options = opts(Some("sonnet"), Some("low"), false, true);
        let (result, seen) = run_collecting(&path, &request("prompt", &options)).await;
        let (answer, usage) = result.unwrap();
        assert_eq!(seen, vec!["Take ", "Olave."]);
        assert!(answer.contains("--model sonnet"), "{answer}");
        assert!(answer.contains("--tools WebSearch"), "{answer}");
        assert!(answer.contains("--effort low"), "{answer}");
        let tmp = std::env::temp_dir();
        let tmp = tmp.canonicalize().unwrap_or(tmp);
        assert!(
            answer.contains(&format!("cwd: {}", tmp.display()))
                || answer.contains(&format!("cwd: {}", std::env::temp_dir().display())),
            "{answer}"
        );
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
        let (result, _) = run_collecting(&path, &request("prompt", &options)).await;
        let err = result.unwrap_err();
        assert!(err.contains("not logged in"), "{err}");
        std::fs::remove_file(path).unwrap();
    }

    #[tokio::test]
    async fn a_cli_that_prints_prose_instead_of_json_is_reported_with_its_head() {
        let path = stub("prose", "#!/bin/sh\ncat >/dev/null\necho 'Take Olave.'\n");
        let options = ChatOptions::default();
        let (result, _) = run_collecting(&path, &request("prompt", &options)).await;
        let err = result.unwrap_err();
        assert!(err.contains("unexpected Claude CLI output"), "{err}");
        assert!(err.contains("Take Olave."), "{err}");
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
        let (result, _) = run_collecting(&path, &req).await;
        let err = result.unwrap_err();
        assert!(err.contains("did not answer"), "{err}");
        std::fs::remove_file(path).unwrap();
    }
}
