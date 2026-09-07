//! What a scoreboard chip shows for a starter whose game has kicked off but
//! who has no entry in Sleeper's per-player points.

mod common;

use draft_assistant_lib::season::build_season_view;

/// The bug: Sleeper leaves a player with nothing to his name out of
/// `players_points`, and the chip fell back to his projection, so a starter
/// who had been on the field for a half without a catch was shown, and
/// counted in the live total, at the fourteen points he was projected for.
#[test]
fn a_started_player_missing_from_the_live_points_shows_zero_not_his_projection() {
    let (loaded, season, config) = common::fixture();
    let before = build_season_view(&loaded, &season, config.my_user_id.as_deref());
    let chip = |view: &draft_assistant_lib::season::SeasonView, game: &str, id: &str| {
        view.live
            .games
            .iter()
            .find(|g| g.game_id == game)
            .and_then(|g| g.chips.iter().find(|c| c.player_id == id))
            .map(|c| c.points)
            .unwrap_or_else(|| panic!("{id} is on the {game} scoreboard"))
    };
    assert!(
        (chip(&before, "g-live", "q1") - 21.5).abs() < 1e-9,
        "the fixture scores him 21.5 mid-game"
    );
    let pre_game_before = chip(&before, "g-pre", "q2");
    assert!(
        pre_game_before > 0.0,
        "a player yet to kick off shows his projection"
    );

    // Sleeper's answer for the same week, with nothing to his name.
    let mut season = season;
    for matchup in std::sync::Arc::make_mut(&mut season.matchups) {
        if let Some(points) = matchup.players_points.as_mut() {
            points.remove("q1");
        }
    }
    let after = build_season_view(&loaded, &season, config.my_user_id.as_deref());

    assert_eq!(
        chip(&after, "g-live", "q1"),
        0.0,
        "a starter on the field with no points has scored nothing, not his projection"
    );
    assert!(
        after.live.totals.my_live_points < before.live.totals.my_live_points,
        "the live total must drop with him"
    );
    assert!(
        (chip(&after, "g-pre", "q2") - pre_game_before).abs() < 1e-9,
        "a player yet to kick off still shows his projection"
    );
}
