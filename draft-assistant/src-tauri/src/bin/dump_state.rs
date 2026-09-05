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

/// What the command line asked for: the league, an optional username to
/// resolve, an optional output file, and how many picks to fake.
type Parsed = (String, Option<String>, Option<String>, u32);

/// Why a command line was refused, so the caller can say which of the two it
/// was rather than printing the usage line for both.
#[derive(Debug, PartialEq)]
enum Refused {
    NoLeague,
    SimulateWithoutANumber,
}

/// The command line, read without touching the process, so what each word
/// means is a thing a test can ask rather than a thing only a run can show.
fn parse_args_from<I: IntoIterator<Item = String>>(args: I) -> Result<Parsed, Refused> {
    let mut positional: Vec<String> = Vec::new();
    let mut simulate = 0u32;
    let mut args = args.into_iter();
    while let Some(arg) = args.next() {
        if arg == "--simulate" {
            simulate = args
                .next()
                .and_then(|n| n.parse().ok())
                .ok_or(Refused::SimulateWithoutANumber)?;
        } else {
            positional.push(arg);
        }
    }
    let league_id = positional.first().cloned().ok_or(Refused::NoLeague)?;
    Ok((
        league_id,
        positional.get(1).cloned(),
        positional.get(2).cloned(),
        simulate,
    ))
}

fn parse_args() -> Parsed {
    parse_args_from(std::env::args().skip(1)).unwrap_or_else(|refused| {
        match refused {
            Refused::SimulateWithoutANumber => eprintln!("--simulate needs a number"),
            Refused::NoLeague => {
                eprintln!("usage: dump_state <league_id> [username] [out.json] [--simulate N]")
            }
        }
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
    let (league_id, username, out_path, simulate) = parse_args();
    let engine = Engine::new(std::env::temp_dir().join("draft-assistant-cli"));

    let mut config = AppConfig::default();
    if let Some(username) = &username {
        config.my_user_id = lookup_user(&engine, username).await;
    }

    let mut loaded = match engine.load_any(&league_id, false, None).await {
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

#[cfg(test)]
mod tests {
    use super::{parse_args_from, Refused};

    fn args(words: &[&str]) -> Result<super::Parsed, Refused> {
        parse_args_from(words.iter().map(|w| w.to_string()))
    }

    #[test]
    fn a_command_line_with_no_league_is_refused() {
        assert_eq!(args(&[]), Err(Refused::NoLeague));
        assert_eq!(args(&["--simulate", "4"]), Err(Refused::NoLeague));
    }

    #[test]
    fn the_bare_words_are_the_league_the_username_and_the_output_in_that_order() {
        assert_eq!(
            args(&["123"]).expect("a league is enough"),
            ("123".to_string(), None, None, 0)
        );
        assert_eq!(
            args(&["123", "mcsleeper26", "out.json"]).expect("parsed"),
            (
                "123".to_string(),
                Some("mcsleeper26".to_string()),
                Some("out.json".to_string()),
                0
            )
        );
    }

    #[test]
    fn simulate_is_read_wherever_it_appears_and_leaves_the_bare_words_alone() {
        let parsed = args(&["--simulate", "12", "123", "mcsleeper26"]).expect("parsed");
        assert_eq!(parsed.0, "123");
        assert_eq!(parsed.1.as_deref(), Some("mcsleeper26"));
        assert_eq!(parsed.3, 12);
    }

    /// `--simulate` with nothing usable after it used to swallow the next
    /// word: `--simulate out.json 123` quietly simulated zero picks and wrote
    /// the dump to a file called `123`.
    #[test]
    fn simulate_without_a_number_is_refused_rather_than_read_as_zero() {
        assert_eq!(
            args(&["123", "--simulate"]),
            Err(Refused::SimulateWithoutANumber)
        );
        assert_eq!(
            args(&["123", "--simulate", "lots"]),
            Err(Refused::SimulateWithoutANumber)
        );
    }
}
