//! When a set lineup is really locked for the week.
//!
//! The screen used to decide this off the scoreboard chips alone: every chip
//! of mine past kickoff meant locked. An empty slot or a starter on bye has no
//! chip, so with the rest of the lineup on the field a FLEX left empty read as
//! locked while the bench player who could still have filled it was there to
//! be started. `live.lineup_locked` is decided in Rust off the lineup itself.

mod common;

use draft_assistant_lib::season::build_season_view;
use draft_assistant_lib::season_api::{GameMeta, ScoreGame};
use std::collections::HashMap;

const WEEK: u32 = 2;

fn live(id: &str, home: &str, away: &str) -> ScoreGame {
    ScoreGame {
        game_id: Some(id.to_string()),
        status: Some("in_game".into()),
        start_time: Some(1_700_000_000_000),
        week: Some(WEEK),
        metadata: Some(GameMeta {
            home_team: Some(home.to_string()),
            away_team: Some(away.to_string()),
            has_started: true,
            is_in_progress: true,
            quarter_num: Some(2),
            ..GameMeta::default()
        }),
    }
}

/// My set lineup with `starters`, and the given games in progress.
fn locked_with(starters: &[&str], games: Vec<ScoreGame>) -> bool {
    let (loaded, mut season, config) = common::fixture();
    let set: Vec<String> = starters.iter().map(|s| (*s).to_string()).collect();
    std::sync::Arc::make_mut(&mut season.rosters)[0].starters = Some(set.clone());
    std::sync::Arc::make_mut(&mut season.matchups)[0].starters = Some(set);
    std::sync::Arc::make_mut(&mut season.matchups)[0].players_points = Some(HashMap::new());
    season.scores = std::sync::Arc::new(games);
    let view = build_season_view(&loaded, &season, config.my_user_id.as_deref());
    view.live.lineup_locked
}

/// The bug: q1, r1 (ATL) and w1 (TB) are on the field, the FLEX is empty, and
/// w2 and r2 (IND) have not kicked off. Every chip of mine was past kickoff,
/// so the screen said locked while either of them could still be started.
#[test]
fn an_empty_slot_with_a_bench_player_still_to_kick_off_is_not_locked() {
    assert!(!locked_with(
        &["q1", "r1", "w1", "0"],
        vec![live("g-atl", "ATL", "TB")]
    ));
}

/// Once the bench has kicked off too, the empty slot can no longer be filled
/// and the lineup really is locked.
#[test]
fn an_empty_slot_nobody_can_fill_any_more_is_locked() {
    assert!(locked_with(
        &["q1", "r1", "w1", "0"],
        vec![live("g-atl", "ATL", "TB"), live("g-ind", "IND", "DAL")]
    ));
}

/// A full lineup with every starter on the field is locked whatever the
/// bench is doing.
#[test]
fn a_full_lineup_on_the_field_is_locked() {
    assert!(locked_with(
        &["q1", "r1", "w1", "w2"],
        vec![live("g-atl", "ATL", "TB"), live("g-ind", "IND", "DAL")]
    ));
}

/// A starter still to kick off can always be benched, so nothing is locked.
#[test]
fn a_starter_still_to_kick_off_keeps_the_lineup_open() {
    assert!(!locked_with(
        &["q1", "r1", "w1", "w2"],
        vec![live("g-atl", "ATL", "TB")]
    ));
}

/// A starter on bye is an empty slot in all but name: open while a bench
/// player who could take the spot has not kicked off.
#[test]
fn a_starter_on_bye_is_not_locked_while_the_bench_can_replace_him() {
    // w5 (DAL) has no projection this week: on bye. r2 (IND) is on the bench.
    assert!(!locked_with(
        &["q1", "r1", "w1", "w5"],
        vec![live("g-atl", "ATL", "TB")]
    ));
}

/// Nothing of mine on the scoreboard is never locked: a bye week, or a
/// scoreboard that has not loaded yet.
#[test]
fn nothing_on_the_scoreboard_is_never_locked() {
    assert!(!locked_with(&["q1", "r1", "w1", "w2"], Vec::new()));
}
