//! How the playoff simulation prices the week that is being played.
//!
//! It used to price it the same way it prices week 14: draw the whole thing
//! from the projection plus noise, every tick, however many of its games were
//! already in the books. So a Sunday evening with your opponent finished and
//! you fifty points clear still read as a coin flip, and the odds jumped
//! around all afternoon on noise nobody could act on.

mod common;

use draft_assistant_lib::engine::LoadedLeague;
use draft_assistant_lib::season::build_season_view;
use draft_assistant_lib::season_api::{GameMeta, ScoreGame};
use draft_assistant_lib::season_engine::LoadedSeason;
use draft_assistant_lib::season_lineup::weekly_lineup_totals;
use draft_assistant_lib::season_lookup::Lookup;
use std::collections::HashMap;

/// The fixture's week, which is the one being played.
const WEEK: u32 = 2;

fn game(id: &str, home: &str, away: &str, meta: GameMeta) -> ScoreGame {
    ScoreGame {
        game_id: Some(id.to_string()),
        status: None,
        start_time: Some(1_700_000_000_000),
        week: Some(WEEK),
        metadata: Some(GameMeta {
            home_team: Some(home.to_string()),
            away_team: Some(away.to_string()),
            ..meta
        }),
    }
}

fn finished() -> GameMeta {
    GameMeta {
        is_over: true,
        has_started: true,
        ..GameMeta::default()
    }
}

fn kicking_off_later() -> GameMeta {
    GameMeta::default()
}

/// Every roster's rest-of-season projection, straight from the lineup solver,
/// with no live pricing anywhere near it. This is the number the standings
/// used to carry and must still carry before anybody kicks off.
fn projection_only(loaded: &LoadedLeague, season: &LoadedSeason) -> HashMap<u32, f64> {
    let lookup = Lookup { loaded };
    let position_of = |id: &str| lookup.position(id);
    let team_of = |id: &str| lookup.team(id);
    let sidelined = |id: &str| lookup.is_sidelined(id);
    season
        .rosters
        .iter()
        .map(|r| {
            let weeks = weekly_lineup_totals(
                &loaded.roster_rules,
                r.player_ids(),
                &position_of,
                &team_of,
                &sidelined,
                &loaded.weekly_points,
                season.week..=loaded.league.last_regular_week(),
            );
            (
                r.roster_id,
                r.settings.points_for() + weeks.iter().map(|(_, p)| p).sum::<f64>(),
            )
        })
        .collect()
}

/// The safety rail on the change: with no game started, pricing the week off
/// the scoreboard has to give back exactly what pricing it off the projection
/// gave, to the last decimal place.
#[test]
fn before_kickoff_the_standings_are_the_projection_only_ones() {
    let (loaded, mut season, config) = common::fixture();
    // Every game still to come. `remaining_by_team` says nothing about a
    // pregame team, which is what makes the live pricing a no-op here.
    season.scores = std::sync::Arc::new(vec![
        game("g1", "ATL", "TB", kicking_off_later()),
        game("g2", "BAL", "PIT", kicking_off_later()),
        game("g3", "IND", "DAL", kicking_off_later()),
    ]);

    let expected = projection_only(&loaded, &season);
    let view = build_season_view(&loaded, &season, config.my_user_id.as_deref());

    assert_eq!(view.standings.len(), 4);
    for row in &view.standings {
        let want = expected[&row.roster_id];
        assert!(
            (row.projected_points - want).abs() < 1e-9,
            "roster {} projected {} but the solver says {want}",
            row.roster_id,
            row.projected_points
        );
    }
}

/// A league where the only game left is mine, and only one team makes the
/// bracket: rosters 1 and 2 are level on record and points, 3 and 4 are a win
/// behind and play each other, so whoever wins 1-v-2 is the one seed and
/// nobody else can catch them.
fn winner_takes_the_only_spot() -> (LoadedLeague, LoadedSeason, Option<String>) {
    let (mut loaded, mut season, config) = common::fixture();
    loaded.league.settings.playoff_teams = Some(1);
    // Week 2 is the last regular week, so week 2 is the whole simulation.
    loaded.league.settings.playoff_week_start = Some(3);

    // Week 2 and nothing after it, so the one game is the whole simulation.
    season.schedule = std::sync::Arc::new(vec![(WEEK, vec![(1, 2), (3, 4)])]);

    for roster in std::sync::Arc::make_mut(&mut season.rosters) {
        let (wins, losses, fpts) = match roster.roster_id {
            1 | 2 => (1, 0, 200.0),
            _ => (0, 1, 100.0),
        };
        roster.settings.wins = wins;
        roster.settings.losses = losses;
        roster.settings.fpts = fpts;
        roster.settings.fpts_decimal = 0.0;
    }
    (loaded, season, config.my_user_id)
}

/// `players_points` giving every one of `ids` `each` points.
fn banked(ids: &[&str], each: f64) -> HashMap<String, f64> {
    ids.iter().map(|id| ((*id).to_string(), each)).collect()
}

/// Both sides finished, so the week can move no further. The simulation has to
/// score it off what was banked, not redraw it from projections and noise.
fn score_the_week(season: &mut LoadedSeason, mine_each: f64, theirs_each: f64) {
    let rows = std::sync::Arc::make_mut(&mut season.matchups);
    rows[0].players_points = Some(banked(&["q1", "r1", "w1", "w2", "r2"], mine_each));
    rows[1].players_points = Some(banked(&["q2", "r3", "w3", "w4"], theirs_each));
    season.scores = std::sync::Arc::new(vec![
        // Roster 1's and roster 2's players, all done.
        game("g-mine", "ATL", "TB", finished()),
        game("g-flex", "IND", "DAL", finished()),
        game("g-theirs", "BAL", "PIT", finished()),
        // Rosters 3 and 4 are still to play, so the week as a whole is not
        // over and the simulation still has a week to run.
        game("g-rest", "SF", "SEA", kicking_off_later()),
    ]);
}

#[test]
fn a_finished_opponent_fifty_behind_is_a_settled_game() {
    let (loaded, mut season, my_user_id) = winner_takes_the_only_spot();
    score_the_week(&mut season, 25.0, 12.5);

    let view = build_season_view(&loaded, &season, my_user_id.as_deref());
    let odds = |roster_id: u32| {
        view.standings
            .iter()
            .find(|row| row.roster_id == roster_id)
            .map(|row| row.playoff_odds)
            .expect("every roster has a row")
    };
    // 100 banked against 50, with nothing left to play in either game.
    assert!(odds(1) > 0.99, "mine read {} rather than certain", odds(1));
    assert!(odds(2) < 0.01, "theirs read {} rather than out", odds(2));
}

/// The mirror image, so the test cannot pass by always saying "you win".
#[test]
fn being_fifty_down_with_the_week_over_is_the_same_certainty_the_other_way() {
    let (loaded, mut season, my_user_id) = winner_takes_the_only_spot();
    score_the_week(&mut season, 12.5, 25.0);

    let view = build_season_view(&loaded, &season, my_user_id.as_deref());
    let mine = view
        .standings
        .iter()
        .find(|row| row.roster_id == 1)
        .expect("my row");
    assert!(mine.playoff_odds < 0.01, "read {}", mine.playoff_odds);
}

/// Every game on the board, over. Used by the two tests below, which are
/// about a week that has been decided rather than one in progress.
fn every_game_final() -> Vec<ScoreGame> {
    [
        "ATL", "TB", "IND", "DAL", "PIT", "BAL", "SF", "SEA", "KC", "DEN",
    ]
    .chunks(2)
    .enumerate()
    .map(|(i, pair)| game(&format!("g{i}"), pair[0], pair[1], finished()))
    .collect()
}

/// The bug: the week being played was priced off the *optimal* lineup, and
/// banked points were then read out of the matchup for whoever that lineup
/// contained. A 30-point afternoon from a player left on the bench was
/// solved into the lineup and then credited in full, so the playoff odds ran
/// on a score the manager had not actually put on the field.
#[test]
fn a_benched_player_who_went_off_is_not_banked_into_my_score() {
    let (loaded, mut season, my_user_id) = winner_takes_the_only_spot();

    // Roster 1 starts q1, r1, w1, w2 and leaves r2 on the bench, where the
    // solver would rather have him: r2 outprojects w2.
    let mine = std::sync::Arc::make_mut(&mut season.matchups);
    mine[0].players_points = Some(HashMap::from([
        ("q1".to_string(), 10.0),
        ("r1".to_string(), 10.0),
        ("w1".to_string(), 10.0),
        ("w2".to_string(), 10.0),
        ("r2".to_string(), 30.0),
    ]));
    mine[1].players_points = Some(banked(&["q2", "r3", "w3", "w4"], 10.0));
    // Both of the teams in this matchup are finished, so every one of those
    // numbers is final and nothing is left to project. Rosters 3 and 4 have
    // not kicked off, so the week as a whole is still open.
    season.scores = std::sync::Arc::new(vec![
        game("g-atl", "ATL", "TB", finished()),
        game("g-ind", "IND", "DAL", finished()),
        game("g-pit", "PIT", "BAL", finished()),
        game("g-rest", "SF", "SEA", kicking_off_later()),
    ]);

    let view = build_season_view(&loaded, &season, my_user_id.as_deref());
    let mine = view
        .standings
        .iter()
        .find(|row| row.roster_id == 1)
        .expect("my row");

    // 200 banked before this week, plus the four starters' ten apiece. The
    // bench thirty belongs to nobody's score.
    assert!(
        (mine.projected_points - 240.0).abs() < 1e-6,
        "projected {} \u{2014} the benched thirty was counted",
        mine.projected_points
    );
}

/// The bug: on a Monday night with every game final, Sleeper's standings have
/// not caught up yet — `settings.wins` still reads last week's. The
/// simulation had already stopped counting the week because its games were
/// over, so the result existed nowhere at all and the odds priced the season
/// as one game shorter than it is.
#[test]
fn a_week_that_is_over_but_not_yet_in_the_standings_still_counts() {
    let (loaded, mut season, config) = common::fixture();
    season.scores = std::sync::Arc::new(every_game_final());
    for roster in std::sync::Arc::make_mut(&mut season.rosters) {
        // One game on the record, in a week 2 that has just finished.
        roster.settings.wins = 1;
        roster.settings.losses = 0;
        roster.settings.ties = 0;
    }
    for matchup in std::sync::Arc::make_mut(&mut season.matchups) {
        matchup.points = match matchup.roster_id {
            1 | 4 => 100.0,
            _ => 50.0,
        };
    }

    let view = build_season_view(&loaded, &season, config.my_user_id.as_deref());
    let row = |id: u32| {
        view.standings
            .iter()
            .find(|row| row.roster_id == id)
            .expect("every roster has a row")
            .clone()
    };

    assert_eq!(row(1).wins, 2, "the win just banked was dropped");
    assert_eq!(row(1).losses, 0);
    assert_eq!(row(2).wins, 1, "the loser keeps his one win");
    assert_eq!(row(2).losses, 1, "and takes the loss");
    assert!(
        (row(1).points_for - 350.0).abs() < 1e-6,
        "points_for read {} \u{2014} this week's hundred never landed",
        row(1).points_for
    );
}
