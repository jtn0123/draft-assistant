//! Everything the app knows about one league once loaded: the league and
//! draft as Sleeper describes them, the scored board, the picks, and the
//! week ahead. Built by `engine::Engine`, read by `view::build_view`.
//! Lifted out of `engine.rs` for the 500-line cap.

use crate::board::BoardPlayer;
use crate::roster::RosterRules;
use crate::sleeper::{
    Draft, League, LeagueRoster, Matchup, NflState, Pick, PlayerMeta, TradedPick, Transaction,
    TrendingPlayer,
};
use crate::valuation::ReplacementModel;
use std::collections::{HashMap, HashSet};

pub struct LoadedLeague {
    pub league: League,
    pub draft: Draft,
    /// user_id -> display name, from /league/{id}/users.
    pub user_names: HashMap<String, String>,
    pub board: Vec<BoardPlayer>,
    pub board_index: HashMap<String, usize>,
    pub replacement_model: ReplacementModel,
    pub roster_rules: RosterRules,
    pub api_picks: Vec<Pick>,
    pub manual_picks: Vec<Pick>,
    /// Draft picks that changed hands, from `/draft/{id}/traded_picks`. Read
    /// with the league and on a forced refresh; a trade made mid-draft shows
    /// up on the next `Refresh data`. Rosters do not depend on it — every
    /// pick names who made it — only who owns the picks still to come.
    pub traded_picks: Vec<TradedPick>,
    /// player_id -> (week, projected points under league scoring), from the
    /// weekly projection rows. What "this week" lineups are built from.
    pub weekly_points: HashMap<String, Vec<(u32, f64)>>,
    /// Where the NFL calendar is, from `/state/nfl`. Absent if the fetch
    /// failed; the week then defaults to the opener.
    pub nfl_state: Option<NflState>,
    /// This week's pairings and the lineups as set on Sleeper. Empty for a
    /// mock draft, before the schedule exists, or if the fetch failed.
    pub matchups: Vec<Matchup>,
    /// Sleeper-wide most-added players, last 24 h. Empty if the fetch failed.
    pub trending: Vec<TrendingPlayer>,
    /// Sleeper's own record and roster per team. Empty for a mock draft.
    pub league_rosters: Vec<LeagueRoster>,
    /// Every completed week's matchups, oldest first, with actual points.
    /// Empty until the regular season has played a week.
    pub past_matchups: Vec<(u32, Vec<Matchup>)>,
    /// Every regular-season week's pairings, played or not. Sleeper
    /// publishes the whole schedule before the season starts.
    pub schedule: Vec<(u32, Vec<Matchup>)>,
    /// Last season, if the league had one and it could be read.
    pub history: Option<crate::history::LeagueHistory>,
    /// Every transaction so far this season, by week. Preseason moves all
    /// sit under week 1.
    pub transactions: Vec<(u32, Vec<Transaction>)>,
    /// Pick numbers known to be keepers: flagged by Sleeper, or sitting in
    /// the book ahead of the draft's progress when first seen. Remembered so
    /// a keeper stays a keeper once the draft passes its slot — the flag
    /// alone cannot be trusted (`Pick::is_keeper`).
    pub keeper_pick_nos: HashSet<u32>,
    pub poll_last_success_at: Option<u64>,
    pub poll_consecutive_failures: u32,
    pub poll_last_error: Option<String>,
    pub players_fetched_at: u64,
    pub projections_fetched_at: u64,
    pub weekly_fetched_at: u64,
    pub warnings: Vec<String>,
    pub player_meta: HashMap<String, PlayerMeta>,
}

impl LoadedLeague {
    /// (name, position, team) for a player id: the board first, then the
    /// player dictionary, then the id itself so nothing renders blank.
    pub fn name_of(&self, player_id: &str) -> (String, String, Option<String>) {
        if let Some(&i) = self.board_index.get(player_id) {
            let p = &self.board[i];
            (p.name.clone(), p.position.clone(), p.team.clone())
        } else if let Some(meta) = self.player_meta.get(player_id) {
            (
                meta.full_name
                    .clone()
                    .unwrap_or_else(|| player_id.to_string()),
                meta.position.clone().unwrap_or_default(),
                meta.team.clone(),
            )
        } else {
            (player_id.to_string(), String::new(), None)
        }
    }
}

#[cfg(test)]
impl LoadedLeague {
    /// A league with nothing in it but what a test puts there.
    pub(crate) fn empty_for_tests() -> Self {
        let league: League = serde_json::from_value(serde_json::json!({
            "league_id": "l", "name": "Test", "season": "2026", "status": "in_season",
            "total_rosters": 2, "roster_positions": ["BN"], "scoring_settings": {}, "draft_id": "d"
        }))
        .unwrap();
        let draft: Draft = serde_json::from_value(serde_json::json!({
            "draft_id": "d", "status": "complete", "type": "snake",
            "settings": {"teams": 2, "rounds": 1}
        }))
        .unwrap();
        Self {
            league,
            draft,
            user_names: HashMap::new(),
            board: Vec::new(),
            board_index: HashMap::new(),
            replacement_model: ReplacementModel::default(),
            roster_rules: RosterRules::new(&["BN".into()]),
            api_picks: Vec::new(),
            manual_picks: Vec::new(),
            traded_picks: Vec::new(),
            weekly_points: HashMap::new(),
            nfl_state: None,
            matchups: Vec::new(),
            trending: Vec::new(),
            league_rosters: Vec::new(),
            past_matchups: Vec::new(),
            transactions: Vec::new(),
            schedule: Vec::new(),
            history: None,
            keeper_pick_nos: HashSet::new(),
            poll_last_success_at: None,
            poll_consecutive_failures: 0,
            poll_last_error: None,
            players_fetched_at: 0,
            projections_fetched_at: 0,
            weekly_fetched_at: 0,
            warnings: Vec::new(),
            player_meta: HashMap::new(),
        }
    }
}
