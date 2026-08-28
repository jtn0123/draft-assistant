//! Headless state dump: loads a league end-to-end and prints the same
//! DraftView JSON the app serves. Doubles as an engine integration test.
//!
//! Usage: dump_state <league_id> [username] [out.json] [--simulate N]
//!
//! --simulate N fakes the first N picks (market drafts by ADP; my own slots
//! take the engine's balanced recommendation) to exercise mid-draft state.

use draft_assistant_lib::draft::slot_for_pick;
use draft_assistant_lib::engine::{build_view, AppConfig, Engine};
use draft_assistant_lib::sleeper::Pick;

fn parse_args() -> (String, Option<String>, Option<String>, u32) {
    let mut positional: Vec<String> = Vec::new();
    let mut simulate = 0u32;
    let mut args = std::env::args().skip(1);
    while let Some(arg) = args.next() {
        if arg == "--simulate" {
            simulate = args
                .next()
                .and_then(|n| n.parse().ok())
                .unwrap_or_else(|| {
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
    let (league_id, username, out_path, simulate) = parse_args();
    let engine = Engine::new(std::env::temp_dir().join("draft-assistant-cli"));

    let mut config = AppConfig::default();
    if let Some(username) = &username {
        config.my_user_id = lookup_user(username).await;
        if config.my_user_id.is_none() {
            eprintln!("warning: Sleeper user '{username}' not found");
        }
    }

    let mut loaded = match engine.load_any(&league_id, false).await {
        Ok(l) => l,
        Err(e) => {
            eprintln!("load failed: {e}");
            std::process::exit(1);
        }
    };

    // Simulated picks: market takes best remaining ADP; my slot takes the
    // engine's balanced recommendation at that moment.
    let teams = loaded.draft.settings.teams;
    for pick_no in 1..=simulate {
        let view = build_view(&loaded, &config);
        let slot = slot_for_pick(pick_no, teams);
        let is_me = view.draft.my_slot == Some(slot);
        let choice = if is_me {
            view.recommendations
                .iter()
                .find(|r| r.mode == "balanced")
                .map(|r| r.player_id.clone())
        } else {
            view.available
                .iter()
                .filter(|p| p.player.adp.is_some())
                .min_by(|a, b| {
                    a.player
                        .adp
                        .partial_cmp(&b.player.adp)
                        .unwrap_or(std::cmp::Ordering::Equal)
                })
                .map(|p| p.player.player_id.clone())
        };
        let Some(player_id) = choice else { break };
        loaded.manual_picks.push(Pick {
            round: (pick_no - 1) / teams + 1,
            pick_no,
            draft_slot: slot,
            player_id,
            picked_by: None,
            metadata: None,
        });
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
