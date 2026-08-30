//! Helpers `build_season_view` leans on: player lookup, the lineup a roster
//! currently has set, and the prose behind a start/sit call.

use crate::engine::LoadedLeague;
use crate::roster::RosterRules;
use crate::season::SeasonView;
use crate::season_api::Matchup;
use crate::season_lineup::LineupSlot;
use crate::season_moves::WaiverTarget;
use crate::season_odds::StandingsRow;
use crate::season_trades::TradeIdea;
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
}

impl SeasonAnalysis {
    /// Lift the reusable parts back out of a freshly built view.
    pub fn of(view: &SeasonView) -> Self {
        Self {
            standings: view.standings.clone(),
            waivers: view.waivers.clone(),
            trades: view.trades.clone(),
        }
    }
}
