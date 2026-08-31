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

/// Recent transactions, with any lineup gaps noticed right now pinned on top.
pub fn activity(
    season: &LoadedSeason,
    rules: &RosterRules,
    lookup: &Lookup,
    team_name: &impl Fn(u32) -> String,
) -> Vec<ActivityItem> {
    let mut activity = season_activity::activity(
        &season.transactions,
        team_name,
        &|id| lookup.name(id),
        ACTIVITY_LIMIT,
    );
    let gaps = season_activity::lineup_gaps(&season.rosters, rules, team_name, season.fetched_at);
    activity.splice(0..0, gaps);
    activity
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
