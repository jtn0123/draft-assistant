//! The shape of a `DraftView` on the wire: every struct the frontend and the
//! model read, and the schema version that gates them.
//!
//! Apart from `view.rs`, which fills these in, so the whole contract can be
//! read in one screen without the assembly around it. A field added here is a
//! field both readers see — that is what `DRAFT_SCHEMA_VERSION` is for.

use crate::board::AvailablePlayer;
use crate::draft::TeamRoster;
use crate::pick_value::PickPrice;
use crate::recommend::Recommendation;
use serde::Serialize;
use std::collections::HashMap;

/// Bumped whenever `DraftView` gains, loses, or renames a serialized field.
/// The frontend gate in `src/api.ts` refuses any other version outright, and
/// `tests/fixture_shape.rs` refuses to let it move without the checked-in
/// `public/dev-fixture.json` moving with it.
pub const DRAFT_SCHEMA_VERSION: &str = "1.2";

#[derive(Debug, Clone, Serialize)]
pub struct DraftStatus {
    pub draft_id: String,
    pub status: String,
    pub teams: u32,
    pub rounds: u32,
    pub pick_timer: Option<u32>,
    pub current_pick: u32,
    pub current_round: u32,
    pub on_clock_slot: u32,
    pub on_clock_name: Option<String>,
    pub my_slot: Option<u32>,
    pub is_my_pick: bool,
    pub picks_until_mine: Option<u32>,
    pub my_next_picks: Vec<u32>,
    pub total_picks_made: usize,
    pub manual_picks_active: bool,
    /// Epoch milliseconds when the current pick's timer expires. Present only
    /// while drafting with a pick timer and a recorded last pick.
    pub clock_deadline_ms: Option<u64>,
    /// Every pick the plain snake gets wrong — because it was traded, or
    /// because the league uses third-round reversal: pick number -> the slot
    /// whose manager makes it. Empty in an ordinary snake league. The
    /// frontend's queue reads this so it never names the wrong manager.
    pub pick_slot_overrides: HashMap<u32, u32>,
    /// Pick numbers held by keepers: already in the book, nobody's turn.
    pub keeper_picks: Vec<u32>,
}

#[derive(Debug, Clone, Serialize)]
pub struct TierAlert {
    pub position: String,
    pub tier: u32,
    pub players_left: u32,
}

#[derive(Debug, Clone, Serialize)]
pub struct RecentPick {
    pub pick_no: u32,
    pub round: u32,
    pub slot: u32,
    pub slot_name: Option<String>,
    pub player_id: String,
    pub name: String,
    pub position: String,
    pub team: Option<String>,
}

/// A position taken `count` times in the last `window` picks.
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct PositionRun {
    pub position: String,
    pub count: u32,
    pub window: u32,
}

#[derive(Debug, Clone, Serialize)]
pub struct DraftView {
    pub schema_version: String,
    pub generated_at: u64,
    pub league: LeagueSummary,
    pub draft: DraftStatus,
    pub my_roster: Option<TeamRoster>,
    pub rosters: Vec<TeamRoster>,
    pub available: Vec<AvailablePlayer>,
    pub tier_alerts: Vec<TierAlert>,
    pub position_run: Option<PositionRun>,
    pub recommendations: Vec<Recommendation>,
    pub recent_picks: Vec<RecentPick>,
    pub replacement_baselines: HashMap<String, f64>,
    /// position -> number of league-wide startable players (incl. flex share)
    pub replacement_demand: HashMap<String, usize>,
    /// What a pick in each round of this draft has been worth, in points over
    /// replacement — empty until the draft has picks to learn from.
    pub pick_prices: Vec<PickPrice>,
    pub data_health: DataHealth,
}

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
