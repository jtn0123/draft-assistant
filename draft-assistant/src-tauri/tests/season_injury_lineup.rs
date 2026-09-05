//! Sleeper leaves a player's weekly projection standing long after the injury
//! report says he is Out. These are the assertions that the lineup solver no
//! longer believes it.

mod common;

use draft_assistant_lib::engine::LoadedLeague;
use draft_assistant_lib::season::{build_season_view, SeasonView};

/// The fixture view, with `player_id` listed at `status` on the board.
fn view_with_injury(player_id: &str, status: Option<&str>) -> SeasonView {
    // The scoreboard is cleared: the fixture's live game is already in its
    // second quarter, and a start/sit call about two players on the field is
    // dropped rather than offered. What this file is about is whether the
    // solver believes a stale projection, which is decided before kickoff.
    let (mut loaded, mut season, config) = common::fixture();
    season.scores.clear();
    if let Some(status) = status {
        mark(&mut loaded, player_id, status);
    }
    build_season_view(&loaded, &season, config.my_user_id.as_deref())
}

fn mark(loaded: &mut LoadedLeague, player_id: &str, status: &str) {
    let index = loaded.board_index[player_id];
    // The board is shared behind an `Arc` so the poll tick can copy the loaded
    // league without duplicating it; a test that edits it takes its own copy.
    std::sync::Arc::make_mut(&mut loaded.board)[index].injury_status = Some(status.to_string());
}

fn projected_points(view: &SeasonView, roster_id: u32) -> f64 {
    view.standings
        .iter()
        .find(|row| row.roster_id == roster_id)
        .map(|row| row.projected_points)
        .expect("every roster is in the standings")
}

#[test]
fn an_opponents_out_starter_stops_inflating_their_score() {
    let healthy = view_with_injury("r3", None);
    // "Rival Back" is a set starter for roster 2 and projects 14.0 a week.
    let injured = view_with_injury("r3", Some("Out"));

    let before = healthy.header.opp_projected;
    let after = injured.header.opp_projected;
    assert!((before - 51.0).abs() < 1e-9, "{before}");
    assert!((after - 37.0).abs() < 1e-9, "{after}");

    // My own side of the comparison is untouched, so the whole of the change
    // in the odds is theirs.
    assert!((healthy.header.my_projected - injured.header.my_projected).abs() < 1e-9);
    assert!(
        injured.header.win_odds_best > healthy.header.win_odds_best,
        "{} should beat {}",
        injured.header.win_odds_best,
        healthy.header.win_odds_best
    );

    // And the rest of their season is priced the same way: an Out player is
    // no longer solved into the best lineup they can field.
    assert!(
        projected_points(&injured, 2) < projected_points(&healthy, 2),
        "rest-of-season projection must drop too"
    );
    // Nobody else's projection moved.
    assert!((projected_points(&injured, 3) - projected_points(&healthy, 3)).abs() < 1e-9);
}

#[test]
fn only_out_and_doubtful_are_zeroed() {
    let healthy = view_with_injury("r3", None);
    // Most Questionable players play, so nothing about the week changes.
    let questionable = view_with_injury("r3", Some("Questionable"));
    assert!((questionable.header.opp_projected - healthy.header.opp_projected).abs() < 1e-9);
    assert!((questionable.header.win_odds_best - healthy.header.win_odds_best).abs() < 1e-9);

    // Doubtful is treated as Out.
    let doubtful = view_with_injury("r3", Some("Doubtful"));
    assert!(doubtful.header.opp_projected < healthy.header.opp_projected);
}

#[test]
fn my_own_out_starter_leaves_the_lineup_i_set_alone() {
    let healthy = view_with_injury("r1", None);
    // "Lead Back" is my set RB at 15.0 a week, with a 10.0 back on my bench.
    let injured = view_with_injury("r1", Some("IR"));

    let mine = injured.matchup.as_ref().expect("my matchup");
    // The lineup I have set is reported exactly as I set it — that number is
    // what I am being asked to change, not a claim about the future.
    let was = healthy.matchup.as_ref().expect("my matchup").set_projected;
    assert!(
        (mine.set_projected - was).abs() < 1e-9,
        "{}",
        mine.set_projected
    );
    assert!((was - 53.0).abs() < 1e-9, "{was}");
    // The best lineup available to me no longer includes him: 55.0 was
    // q1 18 + r1 15 + w1 12 + r2 10; without him it is r2 at RB and the
    // 8.0 receiver in the flex.
    assert!(
        (mine.my_projected - 48.0).abs() < 1e-9,
        "{}",
        mine.my_projected
    );

    // One call, and its gain is still the honest negative: swapping a 15.0
    // projection for a 10.0 one loses points on paper, and saying otherwise
    // would be inventing them.
    assert_eq!(injured.calls.len(), 1, "{:?}", injured.calls);
    let call = &injured.calls[0];
    assert_eq!(call.player_out_id, "r1");
    assert_eq!(call.player_in_id, "r2");
    assert_eq!(call.slot, "RB");
    assert!((call.gain + 5.0).abs() < 1e-9, "{}", call.gain);
    assert!(
        call.reason.as_deref().is_some_and(|r| r.contains("Out")),
        "{:?}",
        call.reason
    );
}

/// The header used to count the calls one way and total their points another:
/// the total was taken before the injury calls joined the list, so a week whose
/// only advice was "your starter is Out" read "1 calls to make, 0.0 points on
/// the table". Count and total now describe the same set.
#[test]
fn the_points_on_the_table_are_the_listed_calls_own_gains() {
    // Lead Back is Out. The bench back projects less than the stale projection
    // still sitting on him, so the point maths raises nothing at all and the
    // only call is the injury one — with a negative gain, which is the whole
    // reason it was left out of the total.
    let view = view_with_injury("r1", Some("Out"));
    assert_eq!(view.calls.len(), 1, "{:?}", view.calls);
    assert_eq!(view.calls[0].player_out_id, "r1");
    assert!(view.calls[0].gain < 0.0, "{}", view.calls[0].gain);

    let listed: f64 = view.calls.iter().map(|call| call.gain).sum();
    assert!(
        (view.points_on_table - listed).abs() < 1e-9,
        "{} points claimed above calls worth {listed}",
        view.points_on_table
    );
}
