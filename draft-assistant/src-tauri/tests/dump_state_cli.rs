//! The headless `dump_state` binary, run for real against the stub Sleeper
//! and the stub Claude: the state dump, `--simulate`, `--ask`/`--chat-out`,
//! and the exit codes a script can rely on.

mod support;

use std::process::Command;
use support::{Fixture, StubSleeper, LEAGUE_ID, MY_USERNAME};

struct Cli {
    stub: StubSleeper,
    dir: std::path::PathBuf,
}

impl Cli {
    fn new(label: &str) -> Self {
        let stub = StubSleeper::start();
        Fixture::load().install(&stub);
        Self {
            stub,
            dir: support::scratch_dir(label),
        }
    }

    fn run(&self, args: &[&str]) -> std::process::Output {
        let claude = support::stub_claude(&self.dir, "Take Fixture QB.");
        Command::new(env!("CARGO_BIN_EXE_dump_state"))
            .args(args)
            .env("DRAFT_ASSISTANT_SLEEPER_BASE", &self.stub.base)
            .env("DRAFT_ASSISTANT_DATA_DIR", &self.dir)
            .env("DRAFT_ASSISTANT_CLAUDE_BIN", &claude)
            .output()
            .expect("dump_state runs")
    }
}

fn stderr(output: &std::process::Output) -> String {
    String::from_utf8_lossy(&output.stderr).into_owned()
}

#[test]
fn prints_the_view_as_json_on_stdout() {
    let cli = Cli::new("stdout");
    let output = cli.run(&[LEAGUE_ID, MY_USERNAME]);
    assert!(output.status.success(), "{}", stderr(&output));
    let view: serde_json::Value = serde_json::from_slice(&output.stdout).unwrap();
    assert_eq!(view["schema_version"], "1.4");
    assert_eq!(view["draft"]["my_slot"], 1, "the username resolved");
    assert_eq!(view["available"].as_array().unwrap().len(), 6);
    assert!(
        cli.dir.join("players.json").is_file(),
        "caches went to the override dir"
    );
}

#[test]
fn writes_a_file_and_simulates_picks() {
    let cli = Cli::new("simulate");
    let out = cli.dir.join("out.json");
    let output = cli.run(&[
        LEAGUE_ID,
        MY_USERNAME,
        out.to_str().unwrap(),
        "--simulate",
        "2",
    ]);
    assert!(output.status.success(), "{}", stderr(&output));
    assert!(stderr(&output).contains("wrote"), "{}", stderr(&output));
    let view: serde_json::Value =
        serde_json::from_str(&std::fs::read_to_string(&out).unwrap()).unwrap();
    assert_eq!(view["draft"]["total_picks_made"], 2);
    assert_eq!(view["draft"]["current_pick"], 3);
    assert_eq!(view["available"].as_array().unwrap().len(), 4);
}

#[test]
fn asks_through_the_chat_path_and_records_the_session() {
    let cli = Cli::new("ask");
    let session = cli.dir.join("session.json");
    let output = cli.run(&[
        LEAGUE_ID,
        MY_USERNAME,
        "--ask",
        "Who should I take?",
        "--ask",
        "Why?",
        "--chat-out",
        session.to_str().unwrap(),
    ]);
    assert!(output.status.success(), "{}", stderr(&output));
    let log = stderr(&output);
    assert!(log.contains("> Who should I take?"), "{log}");
    assert!(log.contains("Take Fixture QB."), "{log}");
    assert!(log.contains("context tokens"), "{log}");
    assert!(
        log.contains("wrote") && log.contains("2 exchanges"),
        "{log}"
    );
    assert!(output.stdout.is_empty(), "no state dump when only asking");
    let recorded: serde_json::Value =
        serde_json::from_str(&std::fs::read_to_string(&session).unwrap()).unwrap();
    let exchanges = recorded.as_array().unwrap();
    assert_eq!(exchanges.len(), 2);
    assert_eq!(exchanges[1]["question"], "Why?");
    assert_eq!(exchanges[1]["answer"], "Take Fixture QB.");
    assert_eq!(exchanges[1]["as_of"]["pick"], 1);
    assert_eq!(exchanges[1]["usage"]["cost_usd"], 0.05);
}

#[test]
fn exit_codes_distinguish_usage_errors_from_load_failures() {
    let cli = Cli::new("exit-codes");
    let usage = cli.run(&[]);
    assert_eq!(usage.status.code(), Some(2));
    assert!(stderr(&usage).contains("usage:"));
    let usage = cli.run(&[LEAGUE_ID, "--ask"]);
    assert_eq!(usage.status.code(), Some(2));
    let usage = cli.run(&[LEAGUE_ID, "--simulate", "many"]);
    assert_eq!(usage.status.code(), Some(2));
    assert!(stderr(&usage).contains("--simulate needs a number"));

    let missing = cli.run(&["no-such-league"]);
    assert_eq!(missing.status.code(), Some(1));
    assert!(
        stderr(&missing).contains("load failed"),
        "{}",
        stderr(&missing)
    );

    let unknown_user = cli.run(&[LEAGUE_ID, "nobody"]);
    assert!(unknown_user.status.success());
    assert!(stderr(&unknown_user).contains("warning: Sleeper user 'nobody' not found"));
}
