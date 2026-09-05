//! Headless companion host: loads a league the way the app does, then serves
//! the phone page and the follower API for it without the desktop window.
//!
//! Usage: companion_host <league_id> [username] [--port N] [--data-dir PATH] [--chat-cli]
//!
//! `--chat-cli` answers shared-chat questions through the Claude Code CLI
//! (subscription, $0) even when an API key is in the Keychain.
//!
//! Meant for trying the phone page in a browser and for driving the follower
//! mode against something real. Prints the address and the pairing code, then
//! runs until interrupted. No poller runs here, so the board is a snapshot.

use draft_assistant_lib::companion::CompanionServer;
use draft_assistant_lib::engine::{AppConfig, Engine};
use draft_assistant_lib::state::{AppState, YahooState};
use std::sync::atomic::{AtomicBool, AtomicU64};
use std::sync::Arc;
use tokio::sync::Mutex;

#[derive(Debug, PartialEq)]
struct Args {
    league_id: String,
    username: Option<String>,
    port: u16,
    data_dir: std::path::PathBuf,
    chat_cli: bool,
}

fn usage() -> ! {
    eprintln!(
        "usage: companion_host <league_id> [username] [--port N] [--data-dir PATH] [--chat-cli]"
    );
    std::process::exit(2);
}

/// Where the default data directory goes when `--data-dir` says nothing. A
/// scratch one, so a throwaway host never writes into the real app's cache.
fn default_data_dir() -> std::path::PathBuf {
    std::env::temp_dir().join("draft-assistant-companion-host")
}

/// The command line, read without touching the process. `None` is "nothing
/// usable was given" -- no league, or a flag left without its value -- which
/// the caller turns into the usage message and exit 2.
fn parse_args_from<I: IntoIterator<Item = String>>(args: I) -> Option<Args> {
    let mut positional: Vec<String> = Vec::new();
    let mut port = draft_assistant_lib::companion::net::DEFAULT_PORT;
    let mut data_dir = default_data_dir();
    let mut chat_cli = false;
    let mut args = args.into_iter();
    while let Some(arg) = args.next() {
        match arg.as_str() {
            "--port" => port = args.next()?.parse().ok()?,
            "--data-dir" => data_dir = args.next()?.into(),
            "--chat-cli" => chat_cli = true,
            _ => positional.push(arg),
        }
    }
    Some(Args {
        league_id: positional.first().cloned()?,
        username: positional.get(1).cloned(),
        port,
        data_dir,
        chat_cli,
    })
}

fn parse_args() -> Args {
    match parse_args_from(std::env::args().skip(1)) {
        Some(args) => args,
        None => usage(),
    }
}

#[tokio::main]
async fn main() {
    let args = parse_args();
    let engine = Arc::new(Engine::new(args.data_dir.clone()));

    let mut config = AppConfig::default();
    if let Some(username) = &args.username {
        match engine.client.user(username).await {
            Ok(user) => config.my_user_id = Some(user.user_id),
            Err(error) => eprintln!("warning: {error}"),
        }
    }
    config.active_league_id = Some(args.league_id.clone());
    if args.chat_cli {
        config.chat_provider = Some("claude_code".to_string());
    }

    let loaded = match engine.load_any(&args.league_id, false, None).await {
        Ok(l) => l,
        Err(e) => {
            eprintln!("load failed: {e}");
            std::process::exit(1);
        }
    };
    eprintln!(
        "{} ({}): {} players on the board",
        loaded.league.name,
        loaded.league.season,
        loaded.board.len()
    );

    let state = Arc::new(AppState {
        engine,
        loaded: Arc::new(Mutex::new(Some(loaded))),
        season: Arc::new(Mutex::new(None)),
        config: Arc::new(Mutex::new(config)),
        polling: Arc::new(AtomicBool::new(false)),
        poll_generation: Arc::new(AtomicU64::new(0)),
        season_polling: Arc::new(AtomicBool::new(false)),
        season_generation: Arc::new(AtomicU64::new(0)),
        last_season_view: Arc::new(Mutex::new(None)),
        yahoo: Arc::new(YahooState::new(Default::default())),
    });

    let host_name = draft_assistant_lib::commands_companion::default_host_name();
    let companion = Arc::new(
        CompanionServer::new(host_name, args.data_dir.clone()).unwrap_or_else(|e| {
            eprintln!("companion failed to build: {e}");
            std::process::exit(1);
        }),
    );
    // There is no webview here; what the app would show on its own screen
    // goes to stderr instead.
    companion.attach(
        state,
        Arc::new(|kind: &str, _payload: serde_json::Value| eprintln!("event: {kind}")),
    );
    let port = match companion.start(args.port).await {
        Ok(port) => port,
        Err(e) => {
            eprintln!("companion failed to start: {e}");
            std::process::exit(1);
        }
    };
    eprintln!(
        "serving on {} — pairing code {}",
        companion.url().unwrap_or_else(|| format!("port {port}")),
        companion.hub.code()
    );
    // Runs until the process is killed; there is nothing to tidy up that the
    // listener's own drop does not cover.
    std::future::pending::<()>().await;
}

#[cfg(test)]
mod tests {
    use super::{default_data_dir, parse_args_from};
    use draft_assistant_lib::companion::net::DEFAULT_PORT;

    fn args(words: &[&str]) -> Option<super::Args> {
        parse_args_from(words.iter().map(|w| w.to_string()))
    }

    /// With no league there is nothing to serve, and a host that started
    /// anyway would show a pairing code for an empty board.
    #[test]
    fn a_command_line_with_no_league_is_refused() {
        assert_eq!(args(&[]), None);
        assert_eq!(args(&["--chat-cli"]), None);
    }

    #[test]
    fn a_league_on_its_own_takes_every_default() {
        let parsed = args(&["123"]).expect("a league is enough");
        assert_eq!(parsed.league_id, "123");
        assert_eq!(parsed.username, None);
        assert_eq!(parsed.port, DEFAULT_PORT);
        assert_eq!(parsed.data_dir, default_data_dir());
        assert!(!parsed.chat_cli);
    }

    #[test]
    fn the_second_bare_word_is_the_username_and_the_flags_are_read_around_it() {
        let parsed = args(&[
            "123",
            "mcsleeper26",
            "--port",
            "9000",
            "--data-dir",
            "/tmp/scratch",
            "--chat-cli",
        ])
        .expect("parsed");
        assert_eq!(parsed.league_id, "123");
        assert_eq!(parsed.username.as_deref(), Some("mcsleeper26"));
        assert_eq!(parsed.port, 9000);
        assert_eq!(parsed.data_dir, std::path::PathBuf::from("/tmp/scratch"));
        assert!(parsed.chat_cli);
    }

    /// A flag left dangling, or given something that is not a port, is a typo
    /// rather than an instruction. Read as a positional it used to become the
    /// league id, and the run failed several seconds later with "league not
    /// found" instead of with the usage line.
    #[test]
    fn a_flag_without_a_usable_value_is_refused_rather_than_guessed_at() {
        assert_eq!(args(&["123", "--port"]), None);
        assert_eq!(args(&["123", "--port", "not-a-port"]), None);
        assert_eq!(args(&["123", "--port", "99999"]), None);
        assert_eq!(args(&["123", "--data-dir"]), None);
    }
}
