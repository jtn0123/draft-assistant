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

async fn lookup_user(username: &str) -> Option<String> {
    #[derive(serde::Deserialize)]
    struct User {
        user_id: String,
    }
    let url = format!("https://api.sleeper.app/v1/user/{username}");
    let user: Option<User> = reqwest::get(&url).await.ok()?.json().await.ok()?;
    user.map(|u| u.user_id)
}

#[tokio::main]
async fn main() {
    let (league_id, username, out_path) = parse_args();
    let engine = Engine::new(std::env::temp_dir().join("draft-assistant-cli"));

    let mut config = AppConfig::default();
    if let Some(username) = &username {
        config.my_user_id = lookup_user(username).await;
        if config.my_user_id.is_none() {
            eprintln!("warning: Sleeper user '{username}' not found");
        }
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
