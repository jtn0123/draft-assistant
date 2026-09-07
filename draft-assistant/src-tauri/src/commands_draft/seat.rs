//! Getting the user's seat confirmed when the load could not.
//!
//! `/league/{id}/users` is the only thing that ties a Sleeper user id to a
//! seat. When that one call fails at load the league still comes up, with no
//! names on the seats and no seat marked as the user's, under
//! [`crate::engine::SEAT_UNCONFIRMED`]. Rather than leave it that way for the
//! evening, every poll tick asks for the list again until it answers.

use crate::engine::{LoadedLeague, SEAT_UNCONFIRMED};
use crate::sleeper::LeagueUser;
use crate::view_types::is_yahoo_key;

/// The league whose member list the next tick should ask for, or `None`
/// when there is nothing to ask: the names are in, the draft is a mock with
/// no league behind it, or the league is Yahoo's, whose members come with
/// the league itself and never from Sleeper.
pub(super) fn users_to_retry(loaded: &LoadedLeague) -> Option<String> {
    let league_id = &loaded.league.league_id;
    (loaded.seat_unconfirmed() && !is_yahoo_key(league_id)).then(|| league_id.clone())
}

/// Take a member list that finally answered. True when it changed anything,
/// which is what tells the poll loop to send a new view.
///
/// The warning the load left behind goes with it: it said "retrying", and
/// this is the retry that worked.
pub(super) fn adopt_users(loaded: &mut LoadedLeague, users: &[LeagueUser]) -> bool {
    if users.is_empty() {
        return false;
    }
    loaded.user_names = crate::sleeper::label_map(users);
    loaded.user_avatars = crate::sleeper::avatar_map(users);
    loaded
        .warnings
        .retain(|warning| !warning.starts_with(SEAT_UNCONFIRMED));
    true
}

#[cfg(test)]
mod tests {
    use super::*;

    fn user(id: &str, name: &str) -> LeagueUser {
        serde_json::from_value(serde_json::json!({"user_id": id, "display_name": name}))
            .expect("a league user")
    }

    fn real_league() -> LoadedLeague {
        let mut loaded = crate::keepers::bare_league("draft-seat");
        loaded.league.league_id = "league-seat".into();
        loaded.user_names.clear();
        loaded
            .warnings
            .push(format!("{SEAT_UNCONFIRMED} (503 from Sleeper)"));
        loaded
    }

    #[test]
    fn a_real_league_with_no_names_asks_again() {
        assert_eq!(
            users_to_retry(&real_league()).as_deref(),
            Some("league-seat")
        );
    }

    #[test]
    fn a_league_whose_names_are_in_does_not() {
        let mut loaded = real_league();
        loaded.user_names.insert("u1".into(), "Ada".into());
        assert_eq!(users_to_retry(&loaded), None);
    }

    #[test]
    fn a_mock_draft_has_no_member_list_to_ask_for() {
        let mut loaded = real_league();
        loaded.league.league_id = loaded.draft.draft_id.clone();
        assert_eq!(users_to_retry(&loaded), None);
    }

    #[test]
    fn a_yahoo_league_is_never_asked_of_sleeper() {
        let mut loaded = real_league();
        loaded.league.league_id = "449.l.12345".into();
        assert_eq!(users_to_retry(&loaded), None);
    }

    #[test]
    fn the_list_that_finally_answers_names_the_seats_and_clears_the_warning() {
        let mut loaded = real_league();
        let changed = adopt_users(&mut loaded, &[user("u1", "Ada"), user("u2", "Bo")]);
        assert!(changed);
        assert_eq!(loaded.user_names.get("u2").map(String::as_str), Some("Bo"));
        assert!(!loaded.seat_unconfirmed());
        assert!(
            loaded
                .warnings
                .iter()
                .all(|w| !w.contains("confirm your seat")),
            "{:?}",
            loaded.warnings
        );
    }

    #[test]
    fn an_empty_answer_changes_nothing_and_keeps_asking() {
        let mut loaded = real_league();
        assert!(!adopt_users(&mut loaded, &[]));
        assert!(loaded.seat_unconfirmed());
        assert_eq!(loaded.warnings.len(), 1);
    }
}
