//! The NFL calendar and one week of a league: what `/state/nfl` and
//! `/league/{id}/matchups/{week}` return. Re-exported from `sleeper` so the
//! client's types read as one family.

use serde::{Deserialize, Serialize};
use std::collections::HashMap;

/// One row of `/league/{id}/rosters`: the record Sleeper keeps for a team,
/// plus its current players. Points are split into whole and hundredths.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct LeagueRoster {
    pub roster_id: u32,
    #[serde(default)]
    pub owner_id: Option<String>,
    #[serde(default)]
    pub settings: RosterSettings,
    #[serde(default)]
    pub starters: Vec<String>,
    #[serde(default)]
    pub players: Vec<String>,
}

#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct RosterSettings {
    #[serde(default)]
    pub wins: u32,
    #[serde(default)]
    pub losses: u32,
    #[serde(default)]
    pub ties: u32,
    #[serde(default)]
    pub fpts: u32,
    #[serde(default)]
    pub fpts_decimal: u32,
    #[serde(default)]
    pub fpts_against: u32,
    #[serde(default)]
    pub fpts_against_decimal: u32,
    #[serde(default)]
    pub waiver_budget_used: u32,
    #[serde(default)]
    pub waiver_position: Option<u32>,
}

/// One row of `/league/{id}/transactions/{week}`. `adds`/`drops` map a
/// player id to the roster it went to / came from.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Transaction {
    pub transaction_id: String,
    #[serde(rename = "type")]
    pub kind: String,
    pub status: String,
    /// ms since the epoch.
    #[serde(default)]
    pub created: u64,
    #[serde(default)]
    pub roster_ids: Vec<u32>,
    #[serde(default)]
    pub adds: Option<HashMap<String, u32>>,
    #[serde(default)]
    pub drops: Option<HashMap<String, u32>>,
    #[serde(default)]
    pub draft_picks: Vec<crate::sleeper::TradedPick>,
    #[serde(default)]
    pub settings: Option<TransactionSettings>,
}

#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct TransactionSettings {
    #[serde(default)]
    pub waiver_bid: Option<u32>,
}

/// One row of `/players/nfl/trending/add`: adds across all of Sleeper in
/// the lookback window. A demand signal, not a value.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct TrendingPlayer {
    pub player_id: String,
    pub count: u64,
}

/// `/state/nfl`: where the NFL calendar is.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct NflState {
    pub week: u32,
    /// "pre" | "regular" | "post" | "off".
    pub season_type: String,
    #[serde(default)]
    pub season: Option<String>,
}

impl NflState {
    /// The fantasy week to plan for: the opener until the regular season
    /// starts, then the current week; nothing once the season is over.
    pub fn upcoming_week(&self) -> Option<u32> {
        match self.season_type.as_str() {
            "pre" | "off" => Some(1),
            "regular" => Some(self.week.max(1)),
            _ => None,
        }
    }
}

/// One roster's side of a week, from `/league/{id}/matchups/{week}`. The
/// two rosters sharing a `matchup_id` play each other. `starters` is the
/// lineup as set on Sleeper, in the league's starting-slot order, with `"0"`
/// for an empty slot.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Matchup {
    pub roster_id: u32,
    #[serde(default)]
    pub matchup_id: Option<u32>,
    #[serde(default)]
    pub starters: Vec<String>,
    #[serde(default)]
    pub players: Vec<String>,
    /// Points scored so far this week (0 before kickoff).
    #[serde(default)]
    pub points: f64,
    /// Every rostered player's points this week under league scoring, once
    /// games have been played. The actuals the season view is built from.
    #[serde(default)]
    pub players_points: HashMap<String, f64>,
}
