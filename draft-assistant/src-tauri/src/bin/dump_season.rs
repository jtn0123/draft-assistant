//! Headless season dump: loads a league end-to-end and prints the same
//! SeasonView JSON the app serves. Doubles as an engine integration test, and
//! produces the fixture the browser preview reads.
//!
//! Usage: dump_season <league_id> [username] [out.json]

use draft_assistant_lib::engine::{AppConfig, Engine};
use draft_assistant_lib::season::build_season_view;
use draft_assistant_lib::season_engine::SeasonLoader;
use draft_assistant_lib::season_history::HistoryStore;

fn parse_args() -> (String, Option<String>, Option<String>) {
    let positional: Vec<String> = std::env::args().skip(1).collect();
    let Some(league_id) = positional.first().cloned() else {
        eprintln!("usage: dump_season <league_id> [username] [out.json]");
        std::process::exit(2);
    };
    (
        league_id,
        positional.get(1).cloned(),
        positional.get(2).cloned(),
    )
}

/// Resolve a username through the engine's own client, so this dump gets the
/// same timeouts, retries and host override as the app does. A bare
/// `reqwest::get` had none of them: no deadline at all, one attempt, and it
/// ignored `DRAFT_ASSISTANT_SLEEPER_BASE`, so a replay run still went to the
/// live API for this one call.
async fn lookup_user(engine: &Engine, username: &str) -> Option<String> {
    match engine.client.user(username).await {
        Ok(user) => Some(user.user_id),
        Err(error) => {
            eprintln!("warning: {error}");
            None
        }
    }
}

#[tokio::main]
async fn main() {
    let (league_id, username, out_path) = parse_args();
    let engine = Engine::new(std::env::temp_dir().join("draft-assistant-cli"));

    let mut config = AppConfig::default();
    if let Some(username) = &username {
        config.my_user_id = lookup_user(&engine, username).await;
    }

    let loaded = match engine.load_any(&league_id, false, None).await {
        Ok(l) => l,
        Err(e) => {
            eprintln!("load failed: {e}");
            std::process::exit(1);
        }
    };
    let mut season = match engine
        .load_season(&loaded.league, config.my_user_id.as_deref(), false)
        .await
    {
        Ok(s) => s,
        Err(e) => {
            eprintln!("season load failed: {e}");
            std::process::exit(1);
        }
    };

    season.history = engine.record_history(&loaded, &season).await;
    let view = build_season_view(&loaded, &season, config.my_user_id.as_deref());
    for warning in &view.data_health.warnings {
        eprintln!("warning: {warning}");
    }
    let json = serde_json::to_string_pretty(&view).expect("serialize");
    match out_path {
        Some(path) => {
            std::fs::write(&path, &json).expect("write failed");
            eprintln!("wrote {path} ({} bytes)", json.len());
        }
        None => println!("{json}"),
    }
}
