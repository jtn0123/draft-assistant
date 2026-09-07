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

use draft_assistant_lib::applog;
use draft_assistant_lib::companion::tls::TlsSource;
use draft_assistant_lib::companion::CompanionServer;
use draft_assistant_lib::engine::{AppConfig, Engine};
use draft_assistant_lib::state::{AppState, YahooState};
use draft_assistant_lib::yahoo_secrets::Item;
use std::path::{Path, PathBuf};
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

/// Point the log at the host's data directory and hand back where it is.
///
/// The failure this prevents: the headless host never initialised the log, so
/// every warning it raised over an evening of phone testing went to a stderr
/// that had scrolled away, and a panic left nothing at all. The same two
/// calls the desktop app makes first thing, then the same first line.
fn start_logging(data_dir: &Path) -> PathBuf {
    applog::init(data_dir.to_path_buf());
    applog::install_panic_hook();
    applog::info(format!(
        "companion_host started version={} platform={} {} data={}",
        env!("CARGO_PKG_VERSION"),
        std::env::consts::OS,
        std::env::consts::ARCH,
        data_dir.display(),
    ));
    data_dir.join(applog::LOG_NAME)
}

/// The headless host's server.
///
/// Its pairings go under [`Item::CompanionDevicesHeadless`], the headless
/// host's own account, not the desktop app's: the two used to share one on
/// a single Mac and overwrite each other's phones. And HTTPS is off: this
/// host is for a browser on the developer's own machine, and a `tailscale
/// cert` run from every throwaway start would count against Let's Encrypt's
/// limits for the real app's name. `build` is the constructor, injected so
/// the test can use the sandboxed one and never touch the Keychain.
fn build_companion(
    host_name: String,
    data_dir: PathBuf,
    build: impl FnOnce(String, PathBuf, Item) -> Result<CompanionServer, String>,
) -> Result<Arc<CompanionServer>, String> {
    let companion = build(host_name, data_dir, Item::CompanionDevicesHeadless)?;
    companion.set_tls(TlsSource::Off);
    Ok(Arc::new(companion))
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
    let log = start_logging(&args.data_dir);
    eprintln!("log: {}", log.display());
    let engine = Arc::new(Engine::for_app(args.data_dir.clone()));

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
        chat_claims: Arc::new(Default::default()),
    });

    let host_name = draft_assistant_lib::commands_companion::default_host_name();
    let companion = build_companion(host_name, args.data_dir.clone(), CompanionServer::new_under)
        .unwrap_or_else(|e| {
            eprintln!("companion failed to build: {e}");
            std::process::exit(1);
        });
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
        "serving on {}, pairing code {}",
        companion.url().unwrap_or_else(|| format!("port {port}")),
        companion.hub.code()
    );
    // Runs until the process is killed; there is nothing to tidy up that the
    // listener's own drop does not cover.
    std::future::pending::<()>().await;
}

#[cfg(test)]
mod tests {
    use super::{build_companion, default_data_dir, parse_args_from, start_logging};
    use draft_assistant_lib::applog;
    use draft_assistant_lib::companion::net::DEFAULT_PORT;
    use draft_assistant_lib::companion::tls::TlsSource;
    use draft_assistant_lib::companion::CompanionServer;
    use draft_assistant_lib::yahoo_secrets::Item;

    /// The two things about the headless host's server that used to be
    /// convention only: its pairings go under the headless account, handed
    /// to the constructor rather than switched on through a process-wide
    /// flag, and HTTPS is off so no throwaway start runs `tailscale cert`.
    #[test]
    fn the_headless_account_is_handed_to_the_constructor_and_https_is_off() {
        let dir = std::env::temp_dir().join(format!(
            "draft-assistant-companion-host-build-{}",
            std::process::id()
        ));
        let _ = std::fs::remove_dir_all(&dir);
        std::fs::create_dir_all(&dir).expect("scratch dir");
        let companion = build_companion(
            "Justin's Mac".to_string(),
            dir.clone(),
            |name, data, item| {
                assert_eq!(
                    item,
                    Item::CompanionDevicesHeadless,
                    "the hub would read the desktop app's device list"
                );
                CompanionServer::sandboxed_under(name, data, item)
            },
        )
        .expect("the companion builds");
        assert_eq!(companion.tls_source(), TlsSource::Off);
        let _ = std::fs::remove_dir_all(&dir);
    }

    /// The headless host used to write no log at all. This is the only test in
    /// this binary that may call `start_logging`: the log directory is set
    /// once per process.
    #[test]
    fn the_headless_host_writes_its_log_under_its_data_dir_and_says_where() {
        let dir = std::env::temp_dir().join(format!(
            "draft-assistant-companion-host-log-{}",
            std::process::id()
        ));
        let _ = std::fs::remove_dir_all(&dir);
        let announced = start_logging(&dir);
        assert_eq!(announced, dir.join(applog::LOG_NAME));
        assert_eq!(
            applog::log_path().as_deref(),
            Some(announced.as_path()),
            "the path printed at start is the one the lines go to"
        );
        let text = std::fs::read_to_string(&announced).expect("the first line created the file");
        assert!(
            text.contains("INFO companion_host started version="),
            "{text}"
        );
        let _ = std::fs::remove_dir_all(&dir);
    }

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
