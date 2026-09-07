//! Whose seat is "mine" when the platform never said.
//!
//! The creator-seat fallback exists for mock drafts, which are joined under
//! a guest id that no config knows. It used to switch on "no member names
//! loaded", which is also what a real league looks like when its `/users`
//! call fails, and then the commissioner's seat lit up green and chimed for
//! a user who was not on the clock.

use super::build_view;
use crate::engine::{AppConfig, LoadedLeague};
use std::collections::HashMap;

/// A two-seat draft created by `creator`, with nobody's name loaded.
fn seated(creator: &str) -> LoadedLeague {
    let mut loaded = crate::keepers::bare_league("draft-seat");
    loaded.draft.settings.teams = 2;
    loaded.draft.settings.rounds = 2;
    loaded.draft.draft_order = Some(HashMap::from([
        (creator.to_string(), 1),
        ("someone-else".to_string(), 2),
    ]));
    loaded.draft.creators = Some(vec![creator.to_string()]);
    loaded.user_names.clear();
    loaded
}

#[test]
fn a_real_league_with_no_member_list_has_no_known_seat() {
    let loaded = seated("commish");
    assert!(
        !loaded.is_mock_draft(),
        "league-1 is not the draft's own id"
    );
    assert!(loaded.seat_unconfirmed());

    let view = build_view(&loaded, &AppConfig::default());
    assert_eq!(view.draft.my_slot, None, "the creator's seat was guessed");
    assert!(!view.draft.is_my_pick, "pick 1 chimed for the commissioner");
    assert!(view.draft.my_next_picks.is_empty());
}

#[test]
fn a_mock_draft_still_falls_back_to_its_creator() {
    let mut loaded = seated("guest-1");
    // What `synthesize_league` leaves behind: the league named by the draft.
    loaded.league.league_id = loaded.draft.draft_id.clone();
    assert!(loaded.is_mock_draft());
    assert!(!loaded.seat_unconfirmed());

    let view = build_view(&loaded, &AppConfig::default());
    assert_eq!(view.draft.my_slot, Some(1));
    assert!(view.draft.is_my_pick, "pick 1 is the creator's in a mock");
}

#[test]
fn a_configured_user_id_wins_on_either_kind_of_draft() {
    let loaded = seated("commish");
    let config = AppConfig {
        my_user_id: Some("someone-else".into()),
        ..AppConfig::default()
    };
    let view = build_view(&loaded, &config);
    assert_eq!(view.draft.my_slot, Some(2));
}
