//! The in-season counterpart to `DraftView`: one struct that is both the UI's
//! data source and the AI-readable state dump, same as the draft side.

use crate::engine::{now_secs, LoadedLeague};
use crate::roster::RosterRules;
use crate::season_activity::{self, ActivityItem};
use crate::season_api::Roster;
use crate::season_deals::{self, TradeDone};
use crate::season_engine::LoadedSeason;
use crate::season_lineup::{
    calls_from_diff, candidates_for, optimal_lineup, Candidate, LineupCall,
};
use crate::season_live::{self, TrackedPlayer};
use crate::season_moves::{self, FreeAgent, RivalRoster, WaiverTarget};
use crate::season_odds::{self, ScheduledGame, StandingsRow, TeamSeason};
use crate::season_trades::TradeIdea;
use crate::season_trends_view::{self, TrendsView};
use crate::view::LeagueSummary;
use serde::Serialize;
use std::collections::{HashMap, HashSet};

pub const SEASON_SCHEMA_VERSION: &str = "1.0";

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

use crate::season_view_parts::{
    current_lineup, matchup_for, opponent_of, trade_ideas_for, why_start, Lookup,
};
// SeasonAnalysis lives beside the other view helpers to stay inside the
// file-size cap, but it belongs to this module's public surface.
pub use crate::season_view_parts::SeasonAnalysis;

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
    let position_of = |id: &str| lookup.position(id);
    let candidates_of = |ids: &[String]| candidates_for(ids, &position_of, weekly, week);
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
    let my_matchup = my_roster_id.and_then(|id| matchup_for(&season.matchups, id));
    let opp_matchup = my_matchup.and_then(|mine| opponent_of(&season.matchups, mine));

    let projected = |id: &str| weekly.get_or_zero(id, week);
    let my_candidates: Vec<Candidate> = my_roster
        .map(|r| candidates_for(r.player_ids(), &position_of, weekly, week))
        .unwrap_or_default();
    let my_optimal = optimal_lineup(rules, &my_candidates);
    let my_current = my_matchup
        .map(|m| current_lineup(loaded, m.starter_ids(), &projected))
        .unwrap_or_else(|| my_optimal.clone());

    let describe = |id: &str| (lookup.name(id), lookup.team(id));
    let reason = |_slot: &str, player_in: &str, player_out: &str| {
        why_start(&lookup, weekly, week, player_in, player_out)
    };
    let eligible = |slot: &str, id: &str| {
        position_of(id).is_some_and(|position| RosterRules::can_fill(slot, &position))
    };
    let calls = calls_from_diff(&my_optimal, &my_current, &eligible, &describe, &reason);
    // Rust's additive identity for f64 is -0.0, so an empty sum serialises as
    // "-0.0" and would render as "−0.0 points on the table". Normalise it.
    let points_on_table: f64 = calls.iter().map(|c| c.gain).sum::<f64>() + 0.0;

    let opp_candidates: Vec<Candidate> = opp_matchup
        .and_then(|m| {
            season
                .rosters
                .iter()
                .find(|r| r.roster_id == m.roster_id)
                .map(|r| candidates_for(r.player_ids(), &position_of, weekly, week))
        })
        .unwrap_or_default();
    let opp_optimal = optimal_lineup(rules, &opp_candidates);
    let opp_current = opp_matchup
        .map(|m| current_lineup(loaded, m.starter_ids(), &projected))
        .unwrap_or_else(|| opp_optimal.clone());

    // The comparison shows my best lineup against their set one: I can change
    // mine, I cannot change theirs.
    let my_projected: f64 = my_optimal.iter().map(|s| s.points).sum::<f64>() + 0.0;
    let opp_projected: f64 = opp_current.iter().map(|s| s.points).sum::<f64>() + 0.0;

    // Both halves of the comparison: my best lineup and the one I have set,
    // each against their set one. The screen toggles between them.
    let rows_against_theirs = |mine: &[crate::season_lineup::LineupSlot]| {
        mine.iter()
            .enumerate()
            .map(|(i, slot)| {
                let theirs = opp_current.get(i);
                let my_id = slot.player_id.clone();
                let opp_id = theirs.and_then(|s| s.player_id.clone());
                let opp_points = theirs.map(|s| s.points).unwrap_or(0.0);
                MatchupRow {
                    slot: slot.slot.clone(),
                    my_name: my_id
                        .as_deref()
                        .map(|id| lookup.name(id))
                        .unwrap_or_default(),
                    my_team: my_id.as_deref().and_then(|id| lookup.team(id)),
                    my_points: slot.points,
                    my_player_id: my_id,
                    opp_name: opp_id
                        .as_deref()
                        .map(|id| lookup.name(id))
                        .unwrap_or_default(),
                    opp_team: opp_id.as_deref().and_then(|id| lookup.team(id)),
                    opp_points,
                    opp_player_id: opp_id,
                    margin: slot.points - opp_points,
                }
            })
            .collect::<Vec<_>>()
    };

    let matchup = my_matchup.map(|mine| MatchupView {
        my_name: team_name(mine.roster_id),
        opp_name: opp_matchup
            .map(|m| team_name(m.roster_id))
            .unwrap_or_else(|| "Bye week".to_string()),
        my_avatar: team_avatar(mine.roster_id),
        opp_avatar: opp_matchup.and_then(|m| team_avatar(m.roster_id)),
        my_projected,
        opp_projected,
        rows: rows_against_theirs(&my_optimal),
        set_rows: rows_against_theirs(&my_current),
        set_projected: my_current.iter().map(|s| s.points).sum(),
    });

    // ---------- standings and odds ----------
    // Rebuilding this means ~1,600 lineup solves plus the playoff
    // simulation. None of it can change from a touchdown being scored,
    // so the poller hands back what it computed on the last real load.
    let standings = match cached {
        Some(analysis) => analysis.standings.clone(),
        None => {
            let last_regular = loaded.league.last_regular_week();
            let teams: Vec<TeamSeason> = season
                .rosters
                .iter()
                .map(|r| TeamSeason {
                    roster_id: r.roster_id,
                    wins: r.settings.wins,
                    losses: r.settings.losses,
                    ties: r.settings.ties,
                    points_for: r.settings.points_for(),
                    weekly_projection: ((week + 1)..=last_regular)
                        .map(|w| {
                            let candidates =
                                candidates_for(r.player_ids(), &position_of, weekly, w);
                            (
                                w,
                                optimal_lineup(rules, &candidates)
                                    .iter()
                                    .map(|s| s.points)
                                    .sum(),
                            )
                        })
                        .collect(),
                })
                .collect();

            let schedule: Vec<ScheduledGame> = season
                .schedule
                .iter()
                .filter(|(w, _)| *w > week)
                .flat_map(|(w, pairs)| {
                    pairs.iter().map(move |(home, away)| ScheduledGame {
                        week: *w,
                        home: *home,
                        away: *away,
                    })
                })
                .collect();

            let playoff_teams = loaded.league.settings.playoff_teams.unwrap_or(6);
            // Seeded from league identity plus how far the season has progressed, so
            // odds stay put between refreshes but do move as results land.
            let seed = season
                .rosters
                .iter()
                .map(|r| r.settings.wins as u64 * 31 + r.settings.fpts as u64)
                .fold(week as u64, |acc, x| {
                    acc.wrapping_mul(1_000_003).wrapping_add(x)
                });
            season_odds::standings(
                &teams,
                &schedule,
                playoff_teams,
                &team_name,
                my_roster_id,
                seed,
            )
        }
    };

    // ---------- live scoreboard ----------
    let mut tracked: Vec<TrackedPlayer> = Vec::new();
    for (matchup, is_mine) in [(my_matchup, true), (opp_matchup, false)] {
        let Some(matchup) = matchup else { continue };
        let lineup = if is_mine { &my_current } else { &opp_current };
        let slot_of: HashMap<&str, &str> = lineup
            .iter()
            .filter_map(|s| Some((s.player_id.as_deref()?, s.slot.as_str())))
            .collect();
        for player_id in matchup.starter_ids() {
            if player_id.is_empty() || player_id == "0" {
                continue;
            }
            tracked.push(TrackedPlayer {
                slot: slot_of
                    .get(player_id.as_str())
                    .map(|s| (*s).to_string())
                    .or_else(|| lookup.position(player_id))
                    .unwrap_or_default(),
                name: lookup.name(player_id),
                team: lookup.team(player_id),
                points: matchup
                    .points_for(player_id)
                    .unwrap_or_else(|| projected(player_id)),
                player_id: player_id.clone(),
                is_mine,
            });
        }
    }
    let games = season_live::live_games(&season.scores, &tracked);
    let windows = season_live::windows(&games);
    let totals = season_live::totals(&games);
    let next_kickoff_ms = season_live::next_window(&windows).map(|w| w.kickoff_ms);

    // ---------- waivers ----------
    let waiver_budget_total = loaded.league.settings.waiver_budget;
    let waiver_budget_left = waiver_budget_total
        .and_then(|budget| my_roster.map(|r| (budget - r.settings.waiver_budget_used).max(0.0)));
    let waivers = match cached {
        Some(analysis) => analysis.waivers.clone(),
        None => {
            let rostered =
                season_moves::rostered_ids(season.rosters.iter().map(Roster::player_ids));
            let free_agents: Vec<FreeAgent> = loaded
                .board
                .iter()
                .filter(|p| !rostered.contains(&p.player_id))
                .map(|p| FreeAgent {
                    player_id: p.player_id.clone(),
                    name: p.name.clone(),
                    position: p.position.clone(),
                    team: p.team.clone(),
                    weekly_points: weekly.get_or_zero(&p.player_id, week),
                })
                .collect();
            let rival_rosters: Vec<RivalRoster> = season
                .rosters
                .iter()
                .filter(|r| Some(r.roster_id) != my_roster_id)
                .map(|r| RivalRoster {
                    roster_id: r.roster_id,
                    player_ids: r.player_ids(),
                })
                .collect();
            season_moves::waiver_targets(
                rules,
                &my_candidates,
                &free_agents,
                &rival_rosters,
                &candidates_of,
                waiver_budget_left,
            )
        }
    };

    // ---------- trades ----------
    let trades = match cached {
        Some(analysis) => analysis.trades.clone(),
        None => trade_ideas_for(
            rules,
            &lookup,
            &season.rosters,
            my_roster_id,
            &my_candidates,
            &candidates_of,
            &team_name,
        ),
    };

    // ---------- my roster ----------
    let starting_ids: HashSet<&str> = my_current
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

    // ---------- feeds ----------
    let mut activity =
        season_activity::activity(&season.transactions, &team_name, &|id| lookup.name(id), 12);
    let gaps = season_activity::lineup_gaps(&season.rosters, rules, &team_name, season.fetched_at);
    activity.splice(0..0, gaps);

    let win_odds = season_odds::win_probability(my_projected, opp_projected);
    let playoff_odds = my_roster_id
        .and_then(|id| standings.iter().find(|s| s.roster_id == id))
        .map(|s| s.playoff_odds)
        .unwrap_or(0.0);

    SeasonView {
        schema_version: SEASON_SCHEMA_VERSION.into(),
        generated_at: now_secs(),
        league: LeagueSummary {
            league_id: loaded.league.league_id.clone(),
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
            opponent_name: opp_matchup.map(|m| team_name(m.roster_id)),
            my_projected,
            opp_projected,
            win_odds,
            playoff_odds,
            locks_in_ms: next_kickoff_ms,
        },
        matchup,
        calls,
        points_on_table,
        waivers,
        waiver_budget_left,
        waiver_budget_total,
        standings,
        live: LiveSection {
            games,
            windows,
            totals,
            next_kickoff_ms,
            bye_teams: season_live::bye_teams(&season.scores),
        },
        roster,
        trades,
        recent_trades: season_deals::recent_trades(
            &season.transactions,
            &team_name,
            &|id| lookup.name(id),
            my_roster_id,
        ),
        activity,
        last_season: season.last_season.clone(),
        trends: season_trends_view::trends_view(
            &season.history,
            &season.transactions,
            &team_name,
            &|id| lookup.name(id),
            my_roster_id,
            40,
        ),
        team_avatars,
        data_health: SeasonHealth {
            fetched_at: season.fetched_at,
            warnings: season.warnings.clone(),
            sources: season.sources.clone(),
        },
        analysis_as_of_secs: analysis_as_of,
    }
}
