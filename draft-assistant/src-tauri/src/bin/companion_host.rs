//! Headless companion host: loads a league the way the app does, then serves
//! the phone page and the follower API for it without the desktop window.
//!
//! Usage: companion_host <league_id> [username] [--port N] [--data-dir PATH]
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

struct Args {
    league_id: String,
    username: Option<String>,
    port: u16,
    data_dir: std::path::PathBuf,
}

fn usage() -> ! {
    eprintln!("usage: companion_host <league_id> [username] [--port N] [--data-dir PATH]");
    std::process::exit(2);
}

fn parse_args() -> Args {
    let mut positional: Vec<String> = Vec::new();
    let mut port = draft_assistant_lib::companion::net::DEFAULT_PORT;
    let mut data_dir = std::env::temp_dir().join("draft-assistant-companion-host");
    let mut args = std::env::args().skip(1);
    while let Some(arg) = args.next() {
        match arg.as_str() {
            "--port" => {
                port = args
                    .next()
                    .and_then(|n| n.parse().ok())
                    .unwrap_or_else(|| usage())
            }
            "--data-dir" => data_dir = args.next().map(Into::into).unwrap_or_else(|| usage()),
            _ => positional.push(arg),
        }
    }
    let Some(league_id) = positional.first().cloned() else {
        usage();
    };
    Args {
        league_id,
        username: positional.get(1).cloned(),
        port,
        data_dir,
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
