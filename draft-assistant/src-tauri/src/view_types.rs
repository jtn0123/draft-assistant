//! The smaller structs of the view: the league summary, data health and the
//! poll's own health. Lifted out of `view.rs` for the 500-line cap.

use crate::board::AvailablePlayer;
use crate::draft::TeamRoster;
use crate::history::LeagueHistory;
use crate::lineup::{ByeWeek, TeamProjection};
use crate::loaded::LoadedLeague;
use crate::matchup::ThisWeek;
use crate::playoffs::TeamOdds;
use crate::recommend::Recommendation;
use crate::results::SeasonSoFar;
use crate::trade::TradeIdea;
use crate::transactions::Activity;
use crate::view::{DraftStatus, RecentPick, TierAlert};
use crate::waivers::WaiverBoard;
use serde::Serialize;
use std::collections::HashMap;

#[derive(Debug, Clone, Serialize)]
pub struct LeagueSummary {
    pub league_id: String,
    pub name: String,
    pub season: String,
    pub total_rosters: u32,
    pub roster_positions: Vec<String>,
    pub draftable_positions: Vec<String>,
    pub scoring_settings: HashMap<String, f64>,
}

#[derive(Debug, Clone, Serialize)]
pub struct DataHealth {
    pub players_fetched_at: u64,
    pub projections_fetched_at: u64,
    pub weekly_fetched_at: u64,
    pub board_size: usize,
    pub warnings: Vec<String>,
    pub poll_last_success_at: Option<u64>,
    pub poll_consecutive_failures: u32,
    pub poll_last_error: Option<String>,
}

#[derive(Debug, Clone, Serialize)]
pub struct PollHealth {
    pub last_success_at: Option<u64>,
    pub consecutive_failures: u32,
    pub last_error: Option<String>,
}

pub fn poll_health(loaded: &LoadedLeague) -> PollHealth {
    PollHealth {
        last_success_at: loaded.poll_last_success_at,
        consecutive_failures: loaded.poll_consecutive_failures,
        last_error: loaded.poll_last_error.clone(),
    }
}

#[derive(Debug, Clone, Serialize)]
pub struct DraftView {
    pub schema_version: String,
    pub generated_at: u64,
    /// Strictly increasing per build. Used by the UI to discard out-of-order
    /// updates; see [`VIEW_SEQ`].
    pub seq: u64,
    pub league: LeagueSummary,
    pub draft: DraftStatus,
    pub my_roster: Option<TeamRoster>,
    pub rosters: Vec<TeamRoster>,
    pub available: Vec<AvailablePlayer>,
    pub tier_alerts: Vec<TierAlert>,
    pub position_run: Option<String>,
    pub recommendations: Vec<Recommendation>,
    pub recent_picks: Vec<RecentPick>,
    /// Every team's best lineup and what it projects to, best first. The
    /// draft's scoreboard: who is winning it so far.
    pub projected_standings: Vec<TeamProjection>,
    /// The week ahead: is my Sleeper lineup the best one, and who do I play.
    /// Absent without a league (mock draft) or before the schedule exists.
    pub this_week: Option<ThisWeek>,
    /// The waiver wire priced for my roster. Only once the draft is over.
    pub waivers: Option<WaiverBoard>,
    /// Record, standings, my results and projected-vs-actual, once a week
    /// of the regular season has been played.
    pub season: Option<SeasonSoFar>,
    /// The league's moves, newest first. Empty for a mock draft.
    pub activity: Vec<Activity>,
    /// One-for-one swaps that lift both my lineup and a rival's. Only once
    /// the draft is over.
    pub trade_ideas: Vec<TradeIdea>,
    /// Simulated rest of season on the league's schedule; empty without one.
    pub playoff_odds: Vec<TeamOdds>,
    /// Last season: who trades, who churns, what claims cost.
    pub history: Option<LeagueHistory>,
    /// My bye weeks, worst first. Empty without a roster.
    pub bye_weeks: Vec<ByeWeek>,
    /// player_id -> projected points for weeks 1..=17 under league scoring
    /// (0 on a bye or with no row), for every rostered player and waiver
    /// target. What a player card draws its season shape from.
    pub player_weeks: HashMap<String, Vec<f64>>,
    pub replacement_baselines: HashMap<String, f64>,
    /// position -> number of league-wide startable players (incl. flex share)
    pub replacement_demand: HashMap<String, usize>,
    pub data_health: DataHealth,
}
