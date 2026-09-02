//! Scoreboard fixtures shared by the tests either side of the
//! `season_live` / `season_live::summary` seam.
//!
//! Their own file so both test modules build their games the same way: a
//! second, drifting copy of "what a live game looks like" would let the two
//! halves of the scoreboard disagree about the input they are tested on.

use crate::season_api::{GameMeta, ScoreGame};
use crate::season_live::TrackedPlayer;

pub fn game(id: &str, away: &str, home: &str, kickoff: i64, meta: GameMeta) -> ScoreGame {
    ScoreGame {
        game_id: Some(id.into()),
        status: None,
        start_time: Some(kickoff),
        week: Some(1),
        metadata: Some(GameMeta {
            away_team: Some(away.into()),
            home_team: Some(home.into()),
            ..meta
        }),
    }
}

pub fn meta_live(quarter: u32, clock: &str, red_zone: &str) -> GameMeta {
    GameMeta {
        quarter_num: Some(quarter),
        time_remaining: Some(clock.into()),
        red_zone: Some(red_zone.into()),
        is_in_progress: true,
        has_started: true,
        away_score: Some(17),
        home_score: Some(20),
        ..Default::default()
    }
}

pub fn meta_final() -> GameMeta {
    GameMeta {
        is_over: true,
        has_started: true,
        away_score: Some(27),
        home_score: Some(13),
        ..Default::default()
    }
}

pub fn meta_pre() -> GameMeta {
    GameMeta::default()
}

pub fn player(id: &str, team: &str, points: f64, is_mine: bool) -> TrackedPlayer {
    TrackedPlayer {
        player_id: id.into(),
        name: id.to_uppercase(),
        slot: "RB".into(),
        team: Some(team.into()),
        points,
        is_mine,
    }
}
