//! Helpers `build_season_view` leans on: player lookup, the lineup a roster
//! currently has set, and the prose behind a start/sit call.

use crate::engine::LoadedLeague;
use crate::roster::RosterRules;
use crate::season::SeasonView;
use crate::season_api::{Matchup, Roster};
use crate::season_injury::{injury_code, PlayerFacts};
use crate::season_lineup::{Candidate, LineupSlot};
use crate::season_moves::WaiverTarget;
use crate::season_odds::StandingsRow;
use crate::season_trades::{self, TradeIdea, TradePartner};
use crate::season_types::MatchupRow;
use crate::weekly::WeeklyPoints;

/// dictionary — DEF entries live only in the latter.
pub struct Lookup<'a> {
    pub loaded: &'a LoadedLeague,
}

impl Lookup<'_> {
    pub fn position(&self, player_id: &str) -> Option<String> {
        if let Some(&i) = self.loaded.board_index.get(player_id) {
            return Some(self.loaded.board[i].position.clone());
        }
        self.loaded
            .player_meta
            .get(player_id)
            .and_then(|m| m.position.clone())
            .filter(|p| !p.is_empty())
    }

    pub fn name(&self, player_id: &str) -> String {
        if let Some(&i) = self.loaded.board_index.get(player_id) {
            return self.loaded.board[i].name.clone();
        }
        self.loaded
            .player_meta
            .get(player_id)
            .and_then(|m| {
                m.full_name.clone().or_else(|| {
                    match (m.first_name.as_ref(), m.last_name.as_ref()) {
                        (Some(f), Some(l)) => Some(format!("{f} {l}")),
                        _ => None,
                    }
                })
            })
            .unwrap_or_else(|| player_id.to_string())
    }

    pub fn team(&self, player_id: &str) -> Option<String> {
        if let Some(&i) = self.loaded.board_index.get(player_id) {
            return self.loaded.board[i].team.clone();
        }
        self.loaded
            .player_meta
            .get(player_id)
            .and_then(|m| m.team.clone())
    }

    /// Sleeper's injury status, as it comes off the player dictionary:
    /// "Questionable", "Out", "IR" and so on. Blank entries read as no status.
    pub fn injury(&self, player_id: &str) -> Option<String> {
        if let Some(&i) = self.loaded.board_index.get(player_id) {
            if let Some(status) = self.loaded.board[i].injury_status.clone() {
                return Some(status).filter(|s| !s.trim().is_empty());
            }
        }
        self.loaded
            .player_meta
            .get(player_id)
            .and_then(|m| m.injury_status.clone())
            .filter(|s| !s.trim().is_empty())
    }
}

impl PlayerFacts for Lookup<'_> {
    fn name(&self, player_id: &str) -> String {
        Lookup::name(self, player_id)
    }
    fn team(&self, player_id: &str) -> Option<String> {
        Lookup::team(self, player_id)
    }
    fn injury_status(&self, player_id: &str) -> Option<String> {
        self.injury(player_id)
    }
}

/// My lineup, slot by slot, against the one the opponent has set — the rows
/// behind both halves of the head-to-head table.
pub fn matchup_rows(
    lookup: &Lookup,
    mine: &[LineupSlot],
    theirs: &[LineupSlot],
) -> Vec<MatchupRow> {
    let describe = |id: Option<&str>| {
        (
            id.map(|id| lookup.name(id)).unwrap_or_default(),
            id.and_then(|id| lookup.team(id)),
            id.and_then(|id| injury_code(lookup.injury(id).as_deref()))
                .map(str::to_string),
        )
    };
    mine.iter()
        .enumerate()
        .map(|(i, slot)| {
            let opp = theirs.get(i);
            let opp_id = opp.and_then(|s| s.player_id.clone());
            let opp_points = opp.map_or(0.0, |s| s.points);
            let (my_name, my_team, my_injury) = describe(slot.player_id.as_deref());
            let (opp_name, opp_team, opp_injury) = describe(opp_id.as_deref());
            MatchupRow {
                slot: slot.slot.clone(),
                my_name,
                my_team,
                my_injury,
                my_points: slot.points,
                my_player_id: slot.player_id.clone(),
                opp_name,
                opp_team,
                opp_injury,
                opp_points,
                opp_player_id: opp_id,
                margin: slot.points - opp_points,
            }
        })
        .collect()
}

pub fn matchup_for(matchups: &[Matchup], roster_id: u32) -> Option<&Matchup> {
    matchups.iter().find(|m| m.roster_id == roster_id)
}

pub fn opponent_of<'a>(matchups: &'a [Matchup], mine: &Matchup) -> Option<&'a Matchup> {
    let id = mine.matchup_id?;
    matchups
        .iter()
        .find(|m| m.matchup_id == Some(id) && m.roster_id != mine.roster_id)
}

/// The lineup a roster currently has set, slot by slot, in league slot order.
pub fn current_lineup(
    loaded: &LoadedLeague,
    starters: &[String],
    points_of: &impl Fn(&str) -> f64,
) -> Vec<LineupSlot> {
    // Sleeper returns starters positionally against roster_positions, with
    // "0" marking an empty slot.
    let starting_slots: Vec<&String> = loaded
        .roster_rules
        .slots()
        .iter()
        .filter(|s| !RosterRules::is_non_starting(s))
        .collect();
    starting_slots
        .iter()
        .enumerate()
        .map(|(i, slot)| {
            let player_id = starters
                .get(i)
                .filter(|id| !id.is_empty() && id.as_str() != "0")
                .cloned();
            let points = player_id.as_deref().map(points_of).unwrap_or(0.0);
            LineupSlot {
                slot: (*slot).clone(),
                player_id,
                points,
            }
        })
        .collect()
}

pub fn why_start(
    lookup: &Lookup,
    weekly: &WeeklyPoints,
    week: u32,
    player_in: &str,
    player_out: &str,
) -> String {
    let in_points = weekly.get(player_in, week).unwrap_or(0.0);
    let out_bye = !player_out.is_empty() && weekly.is_bye(player_out, week);
    if out_bye {
        return format!(
            "{} is on bye this week \u{2014} anyone projected above zero beats an empty slot.",
            lookup.name(player_out)
        );
    }
    if player_out.is_empty() {
        return format!(
            "{} projects {in_points:.1} into a slot you have left empty.",
            lookup.name(player_in)
        );
    }
    let out_points = weekly.get(player_out, week).unwrap_or(0.0);
    format!(
        "{} projects {in_points:.1} against {:.1} for {} \u{2014} a {:+.1} swing on this week's projection.",
        lookup.name(player_in),
        out_points,
        lookup.name(player_out),
        in_points - out_points
    )
}

/// The parts of a season view that cost real time to compute and cannot change
/// from live scoring: rest-of-season projections and playoff odds, waiver
/// targets, and trade ideas.
///
/// Rebuilding these means roughly 1,600 lineup solves plus a playoff
/// simulation plus a trade search — none of which a touchdown can affect. The
/// live poller computes them once and hands them back on every later tick.
#[derive(Debug, Clone)]
pub struct SeasonAnalysis {
    pub standings: Vec<StandingsRow>,
    pub waivers: Vec<WaiverTarget>,
    pub trades: Vec<TradeIdea>,
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
            as_of: view.analysis_as_of_secs,
        }
    }
}

/// Trade ideas against every rival roster.
///
/// Lives here rather than inline in `build_season_view` so that file stays
/// inside the size cap; the shape is exactly the block it replaced.
pub fn trade_ideas_for(
    rules: &RosterRules,
    lookup: &Lookup,
    rosters: &[Roster],
    my_roster_id: Option<u32>,
    my_candidates: &[Candidate],
    candidates_of: &impl Fn(&[String]) -> Vec<Candidate>,
    team_name: &impl Fn(u32) -> String,
) -> Vec<TradeIdea> {
    let partner_candidates: Vec<(u32, String, Vec<Candidate>)> = rosters
        .iter()
        .filter(|r| Some(r.roster_id) != my_roster_id)
        .map(|r| {
            (
                r.roster_id,
                team_name(r.roster_id),
                candidates_of(r.player_ids()),
            )
        })
        .collect();
    let partners: Vec<TradePartner> = partner_candidates
        .iter()
        .map(|(roster_id, name, candidates)| TradePartner {
            roster_id: *roster_id,
            name: name.clone(),
            candidates,
        })
        .collect();
    season_trades::trade_ideas(rules, my_candidates, &partners, &|id| {
        (lookup.name(id), lookup.position(id).unwrap_or_default())
    })
}
