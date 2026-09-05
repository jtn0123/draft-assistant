//! [`super::ask`] against a real child process.
//!
//! The unit tests above cover the pieces in isolation; what is untested there
//! is the part that actually runs: spawning the CLI, writing the prompt to
//! its stdin, and deciding what a non-zero exit means. The stand-in is a
//! shell script, so the arguments and the stdin the app really sends are
//! observable rather than assumed.

use super::*;
use std::io::Write as _;

/// Write an executable shell script into a fresh directory and return its
/// path. The caller removes the directory.
fn fake_cli(label: &str, body: &str) -> PathBuf {
    let dir = std::env::temp_dir().join(format!(
        "draft-assistant-cli-{label}-{}-{}",
        std::process::id(),
        std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .expect("clock")
            .as_nanos()
    ));
    std::fs::create_dir_all(&dir).expect("scratch dir");
    let path = dir.join("claude");
    let mut file = std::fs::File::create(&path).expect("script");
    write!(file, "#!/bin/sh\n{body}\n").expect("write script");
    drop(file);
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        std::fs::set_permissions(&path, std::fs::Permissions::from_mode(0o700)).expect("chmod");
    }
    path
}

fn remove(cli: &Path) {
    if let Some(dir) = cli.parent() {
        std::fs::remove_dir_all(dir).ok();
    }
}

fn question() -> Vec<ChatMessage> {
    vec![ChatMessage {
        role: "user".into(),
        content: "Who is the best value here?".into(),
    }]
}

async fn ask_script(cli: &Path) -> Result<ChatReply, String> {
    ask(
        cli,
        ChatModel::Opus5,
        Effort::High,
        "the board",
        &question(),
    )
    .await
}

#[tokio::test]
async fn a_successful_cli_run_becomes_the_same_reply_the_api_gives() {
    let cli = fake_cli(
        "success",
        r#"cat > /dev/null
echo '{"is_error":false,"result":"Bowers.","usage":{"input_tokens":7,"output_tokens":2},"modelUsage":{"claude-opus-5":{}}}'"#,
    );
    let reply = ask_script(&cli).await.expect("the script answered");
    assert_eq!(reply.text, "Bowers.");
    assert_eq!(reply.input_tokens, 7);
    assert_eq!(reply.output_tokens, 2);
    // The command layer, not this one, decides what a turn cost.
    assert_eq!(reply.cost_usd, 0.0);
    assert!(reply.provider.is_empty());
    remove(&cli);
}

#[tokio::test]
async fn the_question_reaches_the_cli_on_stdin_and_the_board_as_a_flag() {
    // The script records what it was actually given, so the assertions below
    // are about the real invocation rather than a mock's expectations.
    let cli = fake_cli(
        "wiring",
        r#"here=$(dirname "$0")
cat > "$here/stdin.txt"
printf '%s\n' "$@" > "$here/args.txt"
echo '{"is_error":false,"result":"ok"}'"#,
    );
    ask(
        &cli,
        ChatModel::Opus5,
        Effort::High,
        "BOARD-MARKER",
        &question(),
    )
    .await
    .expect("the script answered");

    let here = cli.parent().expect("scratch dir");
    let stdin = std::fs::read_to_string(here.join("stdin.txt")).expect("stdin was written");
    assert_eq!(
        stdin, "Who is the best value here?",
        "the prompt goes over stdin so a long thread never hits ARG_MAX"
    );

    let args = std::fs::read_to_string(here.join("args.txt")).expect("args were written");
    let args: Vec<&str> = args.lines().collect();
    assert!(args.contains(&"-p"), "{args:?}");
    assert!(args.contains(&ChatModel::Opus5.id()), "{args:?}");
    assert!(args.contains(&Effort::High.cli_effort()), "{args:?}");
    // The board is the system prompt, and the CLI's own tools are switched
    // off so it can only read what it was given.
    assert!(
        args.iter().any(|a| a.contains("BOARD-MARKER")),
        "the board must be the system prompt: {args:?}"
    );
    assert!(args.contains(&"--no-session-persistence"), "{args:?}");
    remove(&cli);
}

#[tokio::test]
async fn a_json_error_on_a_failed_exit_is_preferred_to_the_stderr_text() {
    let cli = fake_cli(
        "json-error",
        r#"cat > /dev/null
echo '{"is_error":true,"result":"Credit balance too low"}'
echo 'noise on stderr' >&2
exit 1"#,
    );
    let err = ask_script(&cli).await.expect_err("the script failed");
    assert_eq!(err, "Claude Code: Credit balance too low");
    remove(&cli);
}

#[tokio::test]
async fn a_login_failure_on_stderr_says_what_to_do_about_it() {
    let cli = fake_cli(
        "login",
        r#"cat > /dev/null
echo 'Error: not authenticated' >&2
exit 1"#,
    );
    let err = ask_script(&cli).await.expect_err("the script failed");
    assert!(err.contains("run `claude` in Terminal"), "{err}");
    remove(&cli);
}

#[tokio::test]
async fn a_silent_non_zero_exit_still_names_the_status() {
    let cli = fake_cli("silent", "cat > /dev/null\nexit 42");
    let err = ask_script(&cli).await.expect_err("the script failed");
    assert_eq!(err, "Claude Code exited with status 42");
    remove(&cli);
}

#[tokio::test]
async fn a_cli_that_prints_nonsense_says_so_rather_than_answering() {
    let cli = fake_cli("garbage", "cat > /dev/null\necho 'not json at all'");
    let err = ask_script(&cli).await.expect_err("the output is unusable");
    assert!(err.starts_with("unexpected Claude Code output:"), "{err}");
    remove(&cli);
}

/// The contents of a file the child writes, once it is there.
///
/// The kill and the child's own first instructions run at the same time, and
/// on a machine with every core busy the child can lose that race: the file
/// appears a moment after `ask_within` has already returned. Bounded, so a
/// script that never writes it fails the assertion rather than hanging.
#[cfg(unix)]
fn wait_for_file(path: &Path) -> Option<String> {
    for _ in 0..100 {
        if let Ok(text) = std::fs::read_to_string(path) {
            if !text.trim().is_empty() {
                return Some(text);
            }
        }
        std::thread::sleep(std::time::Duration::from_millis(50));
    }
    None
}

/// True once the process is gone — reaped, or sitting as a zombie waiting to
/// be. Polled, because a kill and its reaping are not instantaneous.
#[cfg(unix)]
fn process_is_gone(pid: &str) -> bool {
    for _ in 0..40 {
        let out = std::process::Command::new("ps")
            .args(["-o", "state=", "-p", pid])
            .output()
            .expect("ps");
        let state = String::from_utf8_lossy(&out.stdout).trim().to_string();
        if state.is_empty() || state.starts_with('Z') {
            return true;
        }
        std::thread::sleep(std::time::Duration::from_millis(50));
    }
    false
}

/// A run that outruns its deadline used to be abandoned: the timeout returned
/// an error to the panel and the CLI kept going, holding a model session open
/// with the app's privileges, one process per impatient question.
#[cfg(unix)]
#[tokio::test]
async fn a_run_that_outlives_its_timeout_is_killed_rather_than_left_running() {
    // The pid is written before anything else and moved into place, so the
    // file either does not exist or holds a whole pid. Recording it after
    // draining stdin -- as this did -- raced the kill: an 800ms deadline on a
    // loaded machine fired while the script was still in `cat`, and the test
    // failed on a missing pid.txt rather than on a surviving process.
    let cli = fake_cli(
        "timeout",
        r#"here=$(dirname "$0")
echo $$ > "$here/pid.tmp"
mv "$here/pid.tmp" "$here/pid.txt"
cat > /dev/null
exec sleep 30"#,
    );
    let err = ask_within(
        &cli,
        ChatModel::Opus5,
        Effort::High,
        "the board",
        &question(),
        Duration::from_millis(800),
    )
    .await
    .expect_err("the script never answers");
    assert!(err.contains("took too long"), "{err}");

    let pid = wait_for_file(&cli.parent().expect("scratch dir").join("pid.txt"))
        .expect("the script recorded its pid");
    let pid = pid.trim();
    assert!(
        process_is_gone(pid),
        "the timed-out CLI (pid {pid}) is still running"
    );
    remove(&cli);
}

/// The prompt goes over a pipe, and a CLI that never reads it fills that pipe
/// and blocks the write. That write sat outside the timeout, so a long
/// question against a wedged CLI hung the panel with no deadline at all.
#[tokio::test]
async fn a_child_that_never_reads_its_stdin_times_out_instead_of_hanging() {
    let cli = fake_cli("deaf", "exec sleep 30");
    let question = vec![ChatMessage {
        role: "user".into(),
        // Comfortably past any pipe buffer, so write_all cannot finish.
        content: "x".repeat(2_000_000),
    }];
    let started = std::time::Instant::now();
    let err = ask_within(
        &cli,
        ChatModel::Opus5,
        Effort::High,
        "the board",
        &question,
        Duration::from_millis(500),
    )
    .await
    .expect_err("nothing is reading the prompt");
    assert!(err.contains("took too long"), "{err}");
    assert!(
        started.elapsed() < Duration::from_secs(10),
        "the write must be inside the deadline, not outside it"
    );
    remove(&cli);
}

#[tokio::test]
async fn a_missing_cli_names_the_path_it_looked_at() {
    let missing = Path::new("/nonexistent/bin/claude");
    let err = ask_script(missing).await.expect_err("nothing to run");
    assert!(err.contains("/nonexistent/bin/claude"), "{err}");
    assert!(err.starts_with("could not start Claude Code"), "{err}");
}

#[tokio::test]
async fn an_empty_thread_is_refused_before_anything_is_spawned() {
    let err = ask(
        Path::new("/nonexistent/bin/claude"),
        ChatModel::Opus5,
        Effort::Off,
        "",
        &[],
    )
    .await
    .expect_err("there is no question");
    assert_eq!(err, "nothing to ask");
}

#[test]
fn an_interrupted_run_reads_as_interrupted_not_as_a_status() {
    assert_eq!(friendly_failure("", None), "Claude Code was interrupted");
    assert_eq!(
        friendly_failure("   ", Some(9)),
        "Claude Code exited with status 9"
    );
}

#[test]
fn only_the_tail_of_a_long_failure_is_shown() {
    let noise = "x".repeat(1000);
    let message = friendly_failure(&format!("{noise}THE-END"), Some(2));
    assert!(message.ends_with("THE-END"), "{message}");
    assert!(message.len() < 400, "the whole log must not be quoted");
}

#[test]
fn looking_for_the_cli_never_returns_something_unrunnable() {
    // Whether the CLI is installed on this machine is not the point: if
    // `find_cli` answers at all, the answer has to be a file that exists.
    if let Some(found) = find_cli() {
        assert!(found.is_file(), "{} is not a file", found.display());
    }
}
