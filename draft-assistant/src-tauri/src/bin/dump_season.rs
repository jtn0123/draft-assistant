//! Headless season dump: loads a league end-to-end and prints the same
//! SeasonView JSON the app serves. Doubles as an engine integration test, and
//! produces the fixture the browser preview reads.
//!
//! Usage: dump_season <league_id> [username] [out.json]

use draft_assistant_lib::engine::{AppConfig, Engine};
use draft_assistant_lib::season::build_season_view;
use draft_assistant_lib::season_engine::SeasonLoader;
use draft_assistant_lib::season_history::HistoryStore;

/// The league, an optional username to resolve, and an optional output file.
type Parsed = (String, Option<String>, Option<String>);

/// The command line, read without touching the process. `None` means no
/// league was named, which is the only way this one can be wrong.
fn parse_args_from<I: IntoIterator<Item = String>>(args: I) -> Option<Parsed> {
    let positional: Vec<String> = args.into_iter().collect();
    Some((
        positional.first().cloned()?,
        positional.get(1).cloned(),
        positional.get(2).cloned(),
    ))
}

fn parse_args() -> Parsed {
    parse_args_from(std::env::args().skip(1)).unwrap_or_else(|| {
        eprintln!("usage: dump_season <league_id> [username] [out.json]");
        std::process::exit(2);
    })
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

    season.history = std::sync::Arc::new(engine.record_history(&loaded, &season).await);
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

#[cfg(test)]
mod tests {
    use super::parse_args_from;

    fn args(words: &[&str]) -> Option<super::Parsed> {
        parse_args_from(words.iter().map(|w| w.to_string()))
    }

    /// Without a league there is nothing to dump, and an empty id would send
    /// a request for `/v1/league/` at Sleeper before anything said so.
    #[test]
    fn a_command_line_with_no_league_is_refused() {
        assert_eq!(args(&[]), None);
    }

    #[test]
    fn the_bare_words_are_the_league_the_username_and_the_output_in_that_order() {
        assert_eq!(
            args(&["123"]).expect("a league is enough"),
            ("123".to_string(), None, None)
        );
        assert_eq!(
            args(&["123", "mcsleeper26"]).expect("parsed"),
            ("123".to_string(), Some("mcsleeper26".to_string()), None)
        );
        assert_eq!(
            args(&["123", "mcsleeper26", "season.json"]).expect("parsed"),
            (
                "123".to_string(),
                Some("mcsleeper26".to_string()),
                Some("season.json".to_string())
            )
        );
        // Anything past the third word is not an argument this takes.
        assert_eq!(
            args(&["123", "mcsleeper26", "season.json", "extra"])
                .expect("parsed")
                .2,
            Some("season.json".to_string())
        );
    }
}
