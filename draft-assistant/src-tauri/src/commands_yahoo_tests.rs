use super::{nonce, sorted_stored, yahoo_leagues_inner};
use crate::state::{AppState, YahooState};
use crate::yahoo::YahooHosts;
use crate::yahoo_types::YahooLeague;
use std::sync::Arc;

fn league(key: &str, name: &str, status: &str) -> YahooLeague {
    YahooLeague {
        league_key: key.to_string(),
        league_id: key.rsplit('.').next().unwrap_or(key).to_string(),
        name: name.to_string(),
        season: "2026".to_string(),
        num_teams: 12,
        draft_status: status.to_string(),
        ..YahooLeague::default()
    }
}

#[test]
fn the_picker_gets_yahoo_leagues_in_a_readable_order() {
    let stored = sorted_stored(vec![
        league("449.l.3", "zeta", "predraft"),
        league("449.l.1", "Alpha", "draft"),
        league("449.l.2", "middle", "postdraft"),
    ]);
    let names: Vec<&str> = stored.iter().map(|l| l.name.as_str()).collect();
    assert_eq!(names, ["Alpha", "middle", "zeta"]);
}

#[test]
fn every_row_says_it_is_a_yahoo_league_and_where_its_draft_has_got_to() {
    let stored = sorted_stored(vec![
        league("449.l.1", "Alpha", "draft"),
        league("449.l.2", "Beta", "postdraft"),
        league("449.l.3", "Gamma", "predraft"),
    ]);
    assert!(stored.iter().all(|l| l.platform == "yahoo"));
    assert_eq!(stored[0].league_id, "449.l.1");
    assert_eq!(stored[0].status.as_deref(), Some("drafting"));
    assert_eq!(stored[1].status.as_deref(), Some("in_season"));
    assert_eq!(stored[2].status.as_deref(), Some("pre_draft"));
}

/// The failure this prevents: a Yahoo command returned `Err`, the string
/// became a toast, the toast was dismissed, and nothing in the log said the
/// command had been called at all.
///
/// `sandboxed` is what keeps this off a developer's real login Keychain: the
/// secrets go in a file in the scratch data directory, where there are none,
/// so the command fails before any network call is made.
#[tokio::test]
async fn a_yahoo_command_that_fails_leaves_an_error_line_naming_it() {
    let (mut state, dir) = AppState::scratch("yahoo-log");
    state.yahoo = Arc::new(YahooState::sandboxed(YahooHosts::default()));
    let capture = crate::applog::Capture::start();
    let out = crate::applog::logged!(
        "yahoo_leagues",
        String::new(),
        yahoo_leagues_inner(&state).await
    );
    assert!(
        out.unwrap_err().contains("Yahoo is not set up"),
        "the sentence the user sees is unchanged"
    );
    assert!(
        capture.saw("ERROR yahoo_leagues failed: Yahoo is not set up"),
        "{:?}",
        capture.lines()
    );
    let _ = std::fs::remove_dir_all(&dir);
}

#[test]
fn two_sign_ins_never_get_the_same_state_to_echo_back() {
    let first = nonce();
    let second = nonce();
    assert_ne!(first, second);
    assert!(!first.is_empty());
    // It goes in a URL query, so it has to survive one unescaped.
    assert!(
        first.chars().all(|c| c.is_ascii_alphanumeric()),
        "{first} is not URL-safe"
    );
}
