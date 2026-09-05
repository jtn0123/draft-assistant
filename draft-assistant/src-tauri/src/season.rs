//! The in-season counterpart to `DraftView`: one struct that is both the UI's
//! data source and the AI-readable state dump, same as the draft side.
//!
//! This module owns the shape of that struct and the order the screen is
//! assembled in; each section is built by its own module and this file only
//! wires them together:
//!
//! - `season_view_matchup` — head-to-head rows and start/sit calls
//! - `season_view_live` — the live scoreboard for those two lineups
//! - `season_view_standings` — standings and playoff odds
//! - `season_view_market` — waiver targets and trade ideas
//! - `season_view_feeds` — activity, completed trades, trends
//!
//! The three expensive sections (standings, waivers, trades) are exactly
//! [`SeasonAnalysis`], which the live poller carries between ticks so a
//! touchdown does not trigger a thousand lineup solves.

use crate::engine::{now_secs, LoadedLeague};
use crate::season_activity::{self, ActivityItem};
use crate::season_api::Roster;
use crate::season_deals::TradeDone;
use crate::season_engine::LoadedSeason;
use crate::season_lineup::LineupCall;
use crate::season_lookup::Lookup;
use crate::season_moves::WaiverTarget;
use crate::season_odds::{self, StandingsRow};
use crate::season_trades::TradeIdea;
use crate::season_trends_view::TrendsView;
use crate::season_view_feeds;
use crate::season_view_live::{self, LiveSide};
use crate::season_view_market;
use crate::season_view_matchup::{self, MatchupSection};
use crate::season_view_standings;
use crate::view::LeagueSummary;
use crate::weekly::WeeklyPoints;
use serde::Serialize;
use std::collections::HashSet;

/// Bumped whenever `SeasonView` gains, loses, or renames a serialized field.
/// Gated in `src/api.ts` and pinned against the checked-in
/// `public/dev-season-fixture.json` by `tests/fixture_shape.rs`.
pub const SEASON_SCHEMA_VERSION: &str = "1.3";

pub use crate::season_sources::{SourceHealth, SourceStatus};
pub use crate::season_types::{
    LastSeasonRow, LiveSection, MatchupRow, MatchupView, RosterRow, SeasonHeader, SeasonHealth,
};

#[derive(Debug, Clone, Serialize)]
pub struct SeasonView {
    pub schema_version: String,
    pub generated_at: u64,
    pub league: LeagueSummary,
    pub week: u32,
    pub season: String,
    pub my_roster_id: Option<u32>,
    pub header: SeasonHeader,
    pub matchup: Option<MatchupView>,
    pub calls: Vec<LineupCall>,
    pub points_on_table: f64,
    pub waivers: Vec<WaiverTarget>,
    pub waiver_budget_left: Option<f64>,
    /// The league's full FAAB budget, so "left" can be shown against it.
    pub waiver_budget_total: Option<f64>,
    pub standings: Vec<StandingsRow>,
    pub live: LiveSection,
    pub roster: Vec<RosterRow>,
    pub trades: Vec<TradeIdea>,
    /// Completed trades this week and last, both sides named.
    pub recent_trades: Vec<TradeDone>,
    pub activity: Vec<ActivityItem>,
    pub last_season: Vec<LastSeasonRow>,
    pub trends: TrendsView,
    /// roster_id -> the manager's avatar reference, for the team pictures.
    /// Only teams whose manager has set one appear.
    pub team_avatars: std::collections::HashMap<u32, String>,
    pub data_health: SeasonHealth,
    /// When the standings, waiver and trade analysis was computed. The live
    /// poll reuses it for minutes at a time, so the screen can admit that.
    pub analysis_as_of_secs: u64,
}

/// The parts of a season view that cost real time to compute and cannot change
/// from live scoring: rest-of-season projections and playoff odds, waiver
/// targets, trade ideas, and the three feed sections.
///
/// Rebuilding these means roughly 1,600 lineup solves plus a playoff
/// simulation plus a trade search, and a diff over forty weeks of history for
/// twelve teams — none of which a touchdown can affect. The live poller
/// computes them once and hands them back on every later tick.
#[derive(Debug, Clone)]
pub struct SeasonAnalysis {
    pub standings: Vec<StandingsRow>,
    pub waivers: Vec<WaiverTarget>,
    pub trades: Vec<TradeIdea>,
    /// The transaction half of the activity feed. The live-facing half — the
    /// empty starter slots — is recomputed every tick and is not in here.
    pub activity: Vec<ActivityItem>,
    pub recent_trades: Vec<TradeDone>,
    pub trends: TrendsView,
    /// Epoch seconds this analysis was computed. Carried with the analysis so
    /// a view built from it reports the age of the ideas it is showing rather
    /// than the moment it happened to be re-serialised.
    pub as_of: u64,
}

impl SeasonAnalysis {
    /// Lift the reusable parts back out of a freshly built view.
    pub fn of(view: &SeasonView) -> Self {
        Self {
            standings: view.standings.clone(),
            waivers: view.waivers.clone(),
            trades: view.trades.clone(),
            // The lineup gaps at the head of the feed came off rosters that
            // the next tick will have refreshed, so they are left behind.
            activity: view
                .activity
                .iter()
                .filter(|item| item.kind != season_activity::LINEUP_KIND)
                .cloned()
                .collect(),
            recent_trades: view.recent_trades.clone(),
            trends: view.trends.clone(),
            as_of: view.analysis_as_of_secs,
        }
    }
}

/// Assemble the whole in-season view.
/// Build the whole view from scratch.
pub fn build_season_view(
    loaded: &LoadedLeague,
    season: &LoadedSeason,
    my_user_id: Option<&str>,
) -> SeasonView {
    build_season_view_cached(loaded, season, my_user_id, None)
}

/// Build a view, optionally reusing the expensive analysis from a previous
/// one. Pass `None` whenever rosters, projections or the schedule may have
/// moved — that is, anywhere except the live-scoring poll.
pub fn build_season_view_cached(
    loaded: &LoadedLeague,
    season: &LoadedSeason,
    my_user_id: Option<&str>,
    cached: Option<&SeasonAnalysis>,
) -> SeasonView {
    let lookup = Lookup { loaded };
    let rules = &loaded.roster_rules;
    let weekly = &loaded.weekly_points;
    let week = season.week;
    // Reused analysis keeps its own build time; a fresh one is being built now.
    let analysis_as_of = cached.map_or_else(now_secs, |analysis| analysis.as_of);

    let my_roster: Option<&Roster> = my_user_id.and_then(|uid| {
        season
            .rosters
            .iter()
            .find(|r| r.owner_id.as_deref() == Some(uid))
    });
    let my_roster_id = my_roster.map(|r| r.roster_id);

    let team_name = |roster_id: u32| -> String {
        season
            .rosters
            .iter()
            .find(|r| r.roster_id == roster_id)
            .and_then(|r| r.owner_id.as_ref())
            .and_then(|owner| loaded.user_names.get(owner).cloned())
            .unwrap_or_else(|| format!("Team {roster_id}"))
    };

    let team_avatar = |roster_id: u32| -> Option<String> {
        season
            .rosters
            .iter()
            .find(|r| r.roster_id == roster_id)
            .and_then(|r| r.owner_id.as_ref())
            .and_then(|owner| loaded.user_avatars.get(owner).cloned())
    };
    let team_avatars: std::collections::HashMap<u32, String> = season
        .rosters
        .iter()
        .filter_map(|r| team_avatar(r.roster_id).map(|a| (r.roster_id, a)))
        .collect();

    // ---------- this week's matchup ----------
    let head_to_head = season_view_matchup::build_matchup(
        loaded,
        season,
        &lookup,
        my_roster,
        &team_name,
        &team_avatar,
    );

    // ---------- standings and odds ----------
    // Rebuilding this means ~1,600 lineup solves plus the playoff
    // simulation. None of it can change from a touchdown being scored,
    // so the poller hands back what it computed on the last real load.
    let standings = match cached {
        Some(analysis) => analysis.standings.clone(),
        None => {
            season_view_standings::standings_rows(loaded, season, &lookup, my_roster_id, &team_name)
        }
    };

    // ---------- live scoreboard ----------
    let live = season_view_live::live_section(
        season,
        &lookup,
        weekly,
        LiveSide {
            matchup: head_to_head.my_matchup,
            lineup: &head_to_head.my_current,
        },
        LiveSide {
            matchup: head_to_head.opp_matchup,
            lineup: &head_to_head.opp_current,
        },
    );
    let next_kickoff_ms = live.next_kickoff_ms;

    // ---------- waivers and trades ----------
    let waiver_budget_total = loaded.league.settings.waiver_budget;
    let waiver_budget_left = waiver_budget_total
        .and_then(|budget| my_roster.map(|r| (budget - r.settings.waiver_budget_used).max(0.0)));
    let waivers = match cached {
        Some(analysis) => analysis.waivers.clone(),
        None => season_view_market::waiver_targets(
            loaded,
            season,
            &lookup,
            my_roster_id,
            &head_to_head.my_candidates,
            waiver_budget_left,
        ),
    };
    let trades = match cached {
        Some(analysis) => analysis.trades.clone(),
        None => season_view_market::trade_ideas(
            loaded,
            season,
            &lookup,
            my_roster_id,
            &head_to_head.my_candidates,
            &team_name,
        ),
    };

    // ---------- my roster ----------
    let roster = roster_rows(season, &lookup, weekly, week, my_roster, &head_to_head);

    // ---------- feeds ----------
    // All three read only the transaction log and the league history, both set
    // once at load, so a cached analysis carries them. The empty-starter-slot
    // items at the head of the feed are the exception: they come off rosters
    // the live poll refreshes, so they are gathered fresh every time.
    let mut activity = season_view_feeds::lineup_gaps(season, rules, &team_name);
    activity.extend(match cached {
        Some(analysis) => analysis.activity.clone(),
        None => season_view_feeds::transaction_activity(season, &lookup, &team_name),
    });
    let recent_trades = match cached {
        Some(analysis) => analysis.recent_trades.clone(),
        None => season_view_feeds::recent_trades(season, &lookup, my_roster_id, &team_name),
    };
    let trends = match cached {
        Some(analysis) => analysis.trends.clone(),
        None => season_view_feeds::trends(season, &lookup, my_roster_id, &team_name),
    };

    // Priced twice off the same opponent: once for the lineup I should be
    // starting, once for the one I have. The screen shows whichever it is
    // showing the lineup for.
    let win_odds_best =
        season_odds::win_probability(&head_to_head.my_spread, &head_to_head.opp_spread);
    let win_odds_set =
        season_odds::win_probability(&head_to_head.my_set_spread, &head_to_head.opp_spread);
    let my_standing = my_roster_id.and_then(|id| standings.iter().find(|s| s.roster_id == id));
    let playoff_odds = my_standing.map_or(0.0, |s| s.playoff_odds);
    let playoff_status = my_standing.and_then(|s| s.playoff_status.clone());

    SeasonView {
        schema_version: SEASON_SCHEMA_VERSION.into(),
        generated_at: now_secs(),
        league: LeagueSummary {
            league_id: loaded.league.league_id.clone(),
            platform: crate::view_types::platform_for(&loaded.league.league_id).to_string(),
            name: loaded.league.name.clone(),
            season: loaded.league.season.clone(),
            total_rosters: loaded.league.total_rosters,
            roster_positions: loaded.league.roster_positions.clone(),
            draftable_positions: rules.draftable_positions(),
            scoring_settings: loaded.league.scoring_settings.clone(),
        },
        week,
        season: loaded.league.season.clone(),
        my_roster_id,
        header: SeasonHeader {
            opponent_name: head_to_head.opp_matchup.map(|m| team_name(m.roster_id)),
            my_projected: head_to_head.my_projected,
            my_set_projected: head_to_head.my_set_projected,
            opp_projected: head_to_head.opp_projected,
            win_odds_best,
            win_odds_set,
            playoff_odds,
            playoff_status,
            locks_in_ms: next_kickoff_ms,
        },
        matchup: head_to_head.matchup,
        calls: head_to_head.calls,
        points_on_table: head_to_head.points_on_table,
        waivers,
        waiver_budget_left,
        waiver_budget_total,
        standings,
        live,
        roster,
        trades,
        recent_trades,
        activity,
        last_season: season.last_season.as_ref().clone(),
        trends,
        team_avatars,
        data_health: SeasonHealth {
            fetched_at: season.fetched_at,
            warnings: season.warnings.clone(),
            sources: season.sources.clone(),
        },
        analysis_as_of_secs: analysis_as_of,
    }
}

/// My whole roster, starters first, then byes, then the bench — each group by
/// points scored so far.
fn roster_rows(
    season: &LoadedSeason,
    lookup: &Lookup,
    weekly: &WeeklyPoints,
    week: u32,
    my_roster: Option<&Roster>,
    head_to_head: &MatchupSection,
) -> Vec<RosterRow> {
    let starting_ids: HashSet<&str> = head_to_head
        .my_current
        .iter()
        .filter_map(|s| s.player_id.as_deref())
        .collect();
    let mut roster: Vec<RosterRow> = my_roster
        .map(|r| {
            r.player_ids()
                .iter()
                .map(|id| RosterRow {
                    role: if weekly.is_bye(id, week) {
                        "Bye".to_string()
                    } else if starting_ids.contains(id.as_str()) {
                        "Start".to_string()
                    } else {
                        "Bench".to_string()
                    },
                    name: lookup.name(id),
                    position: lookup.position(id).unwrap_or_default(),
                    team: lookup.team(id),
                    points: season.season_points.get(id).copied().unwrap_or(0.0),
                    projected: weekly.get_or_zero(id, week),
                    player_id: id.clone(),
                })
                .collect()
        })
        .unwrap_or_default();
    roster.sort_by(|a, b| {
        let rank = |role: &str| match role {
            "Start" => 0,
            "Bye" => 1,
            _ => 2,
        };
        rank(&a.role)
            .cmp(&rank(&b.role))
            .then_with(|| b.points.total_cmp(&a.points))
    });
    roster
}
