//! Headless state dump: loads a league end-to-end and prints the same
//! DraftView JSON the app serves. Doubles as an engine integration test.
//!
//! Usage: dump_state <league_id> [username] [out.json] [--simulate N]
//!
//! --simulate N fakes the first N picks (market drafts by ADP; my own slots
//! take the engine's balanced recommendation) to exercise mid-draft state.

use draft_assistant_lib::engine::{AppConfig, Engine};
use draft_assistant_lib::simulation::apply_simulated_pick;
use draft_assistant_lib::view::build_view;

fn parse_args() -> (String, Option<String>, Option<String>, u32) {
    let mut positional: Vec<String> = Vec::new();
    let mut simulate = 0u32;
    let mut args = std::env::args().skip(1);
    while let Some(arg) = args.next() {
        if arg == "--simulate" {
            simulate = args.next().and_then(|n| n.parse().ok()).unwrap_or_else(|| {
                eprintln!("--simulate needs a number");
                std::process::exit(2);
            });
        } else {
            positional.push(arg);
        }
    }
    let Some(league_id) = positional.first().cloned() else {
        eprintln!("usage: dump_state <league_id> [username] [out.json] [--simulate N]");
        std::process::exit(2);
    };
    (
        league_id,
        positional.get(1).cloned(),
        positional.get(2).cloned(),
        simulate,
    )
}

#[tokio::main]
async fn main() {
    let (league_id, username, out_path, simulate) = parse_args();
    let engine = match Engine::new(std::env::temp_dir().join("draft-assistant-cli")) {
        Ok(engine) => engine,
        Err(e) => {
            eprintln!("error: {e}");
            std::process::exit(1);
        }
    };

    let mut config = AppConfig::default();
    if let Some(username) = &username {
        match engine.client.user_id(username).await {
            Ok(Some(user_id)) => config.my_user_id = Some(user_id),
            Ok(None) => eprintln!("warning: Sleeper user '{username}' not found"),
            Err(error) => eprintln!("warning: could not look up '{username}': {error}"),
        }
    }

    let mut loaded = match engine.load_any(&league_id, false).await {
        Ok(l) => l,
        Err(e) => {
            eprintln!("load failed: {e}");
            std::process::exit(1);
        }
    };

    for pick_no in 1..=simulate {
        if apply_simulated_pick(&mut loaded, &config, pick_no).is_none() {
            break;
        }
    }

    let view = build_view(&loaded, &config);
    let json = serde_json::to_string_pretty(&view).expect("serialize");
    match out_path {
        Some(path) => {
            std::fs::write(&path, &json).expect("write failed");
            eprintln!("wrote {path} ({} bytes)", json.len());
        }
        None => println!("{json}"),
    }
}
