//! The what-the-league-has-been-doing section of the season view: the recent
//! activity feed, completed trades, and the longer-run trends panel.
//!
//! All three read the same two sources — the transaction log and the league
//! history — and differ only in how far back they look and what they keep.

use crate::roster::RosterRules;
use crate::season_activity::{self, ActivityItem};
use crate::season_deals::{self, TradeDone};
use crate::season_engine::LoadedSeason;
use crate::season_lookup::Lookup;
use crate::season_trends_view::{self, TrendsView};

/// How many transactions the activity feed shows.
const ACTIVITY_LIMIT: usize = 12;
/// How many weeks of history the trends panel reaches back over.
const TRENDS_LIMIT: usize = 40;

/// The transaction half of the activity feed. Transactions only arrive with a
/// full load, so this is what the analysis cache carries between ticks.
pub fn transaction_activity(
    season: &LoadedSeason,
    lookup: &Lookup,
    team_name: &impl Fn(u32) -> String,
) -> Vec<ActivityItem> {
    season_activity::activity(
        &season.transactions,
        team_name,
        &|id| lookup.name(id),
        &|id| lookup.team(id),
        ACTIVITY_LIMIT,
    )
}

/// The empty starting slots there are right now, which lead the feed.
///
/// Deliberately not cached: rosters are refreshed on every live tick, and a
/// frozen "you have an empty slot" warning is worse than none at all. It is a
/// dozen rosters, so recomputing it costs nothing.
pub fn lineup_gaps(
    season: &LoadedSeason,
    rules: &RosterRules,
    team_name: &impl Fn(u32) -> String,
) -> Vec<ActivityItem> {
    season_activity::lineup_gaps(&season.rosters, rules, team_name, season.fetched_at)
}

/// Completed trades this week and last, both sides named.
pub fn recent_trades(
    season: &LoadedSeason,
    lookup: &Lookup,
    my_roster_id: Option<u32>,
    team_name: &impl Fn(u32) -> String,
) -> Vec<TradeDone> {
    season_deals::recent_trades(
        &season.transactions,
        team_name,
        &|id| lookup.name(id),
        my_roster_id,
    )
}

/// The trends panel: results and moves over the league's recorded history.
pub fn trends(
    season: &LoadedSeason,
    lookup: &Lookup,
    my_roster_id: Option<u32>,
    team_name: &impl Fn(u32) -> String,
) -> TrendsView {
    season_trends_view::trends_view(
        &season.history,
        &season.transactions,
        team_name,
        &|id| lookup.name(id),
        my_roster_id,
        TRENDS_LIMIT,
    )
}
