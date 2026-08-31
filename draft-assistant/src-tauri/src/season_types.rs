//! The plain data structs of the season view, kept apart from the assembly
//! logic in `season.rs` so each file stays readable.

use crate::season_live::{KickoffWindow, LiveGame, LiveTotals};
use crate::season_sources::SourceHealth;
use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, Serialize)]
pub struct SeasonHeader {
    pub opponent_name: Option<String>,
    pub my_projected: f64,
    pub opp_projected: f64,
    /// 0.0..=1.0 chance of winning this week.
    pub win_odds: f64,
    /// 0.0..=1.0 chance of making the playoff bracket.
    pub playoff_odds: f64,
    /// Epoch milliseconds of the next kickoff involving one of my starters.
    pub locks_in_ms: Option<i64>,
}

#[derive(Debug, Clone, Serialize)]
pub struct MatchupRow {
    pub slot: String,
    pub my_player_id: Option<String>,
    pub my_name: String,
    pub my_team: Option<String>,
    pub my_points: f64,
    pub opp_player_id: Option<String>,
    pub opp_name: String,
    pub opp_team: Option<String>,
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
