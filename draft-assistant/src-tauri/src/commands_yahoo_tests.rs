use super::{nonce, sorted_stored};
use crate::yahoo_types::YahooLeague;

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
