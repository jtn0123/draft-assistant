//! The plain data structs of the season view, kept apart from the assembly
//! logic in `season.rs` so each file stays readable.

use crate::season_live::{KickoffWindow, LiveGame, LiveTotals};
use crate::season_sources::SourceHealth;
use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, Serialize)]
pub struct SeasonHeader {
    pub opponent_name: Option<String>,
    /// What my best lineup projects, and what the one I have set on Sleeper
    /// projects. The screen picks between them with the same toggle that
    /// picks between the two win odds below, so the score it prints and the
    /// percentage beside it are always readings of one lineup.
    pub my_projected: f64,
    pub my_set_projected: f64,
    pub opp_projected: f64,
    /// 0.0..=1.0 chance of winning this week with the best lineup available,
    /// and with the one actually set. They differ exactly when the set lineup
    /// is leaving points on the bench.
    pub win_odds_best: f64,
    pub win_odds_set: f64,
    /// 0.0..=1.0 chance of making the playoff bracket. Only a forecast during
    /// the regular season: once `playoff_status` is set the bracket is cut and
    /// the percentage is a flat 1 or 0, which the screen must not print.
    pub playoff_odds: f64,
    /// "In the playoffs — seed 3" or "Missed the playoffs", once the regular
    /// season is over. `None` while the percentage still means something.
    #[serde(default)]
    pub playoff_status: Option<String>,
    /// Epoch milliseconds of the next kickoff involving one of my starters.
    pub locks_in_ms: Option<i64>,
}

#[derive(Debug, Clone, Serialize)]
pub struct MatchupRow {
    pub slot: String,
    pub my_player_id: Option<String>,
    pub my_name: String,
    pub my_team: Option<String>,
    /// "Q", "D" or "O" when the player carries an injury tag this week.
    #[serde(default)]
    pub my_injury: Option<String>,
    pub my_points: f64,
    pub opp_player_id: Option<String>,
    pub opp_name: String,
    pub opp_team: Option<String>,
    #[serde(default)]
    pub opp_injury: Option<String>,
    pub opp_points: f64,
    pub margin: f64,
}

#[derive(Debug, Clone, Serialize)]
pub struct MatchupView {
    pub my_name: String,
    pub opp_name: String,
    /// Manager pictures for the two teams, when they have one.
    pub my_avatar: Option<String>,
    pub opp_avatar: Option<String>,
    /// What my best lineup would score.
    pub my_projected: f64,
    pub opp_projected: f64,
    /// My best lineup, slot by slot, against their set one.
    pub rows: Vec<MatchupRow>,
    /// The lineup I actually have set, same comparison.
    pub set_rows: Vec<MatchupRow>,
    /// What the lineup I have set would score.
    pub set_projected: f64,
}

#[derive(Debug, Clone, Serialize)]
pub struct RosterRow {
    pub player_id: String,
    pub name: String,
    pub position: String,
    pub team: Option<String>,
    /// "Start", "Bench", or "Bye".
    pub role: String,
    /// Season-to-date fantasy points.
    pub points: f64,
    /// Projected points this week (0 on a bye).
    pub projected: f64,
}

#[derive(Debug, Clone, Serialize)]
pub struct LiveSection {
    pub games: Vec<LiveGame>,
    pub windows: Vec<KickoffWindow>,
    pub totals: LiveTotals,
    pub next_kickoff_ms: Option<i64>,
    /// NFL teams idle this week; empty when no schedule has loaded.
    pub bye_teams: Vec<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct LastSeasonRow {
    pub place: u32,
    pub name: String,
    pub record: String,
    pub points: f64,
    /// "Champ", "Most pts", or nothing.
    pub tag: Option<String>,
    pub is_mine: bool,
}

#[derive(Debug, Clone, Serialize)]
pub struct SeasonHealth {
    pub fetched_at: u64,
    pub warnings: Vec<String>,
    /// Freshness one source at a time, so a badge can be green about the
    /// scoreboard and honest about the rosters at the same time.
    pub sources: SourceHealth,
}
