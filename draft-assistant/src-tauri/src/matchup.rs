//! This week: is the lineup set on Sleeper the best one, and how does it
//! stack up against the opponent?
//!
//! Sleeper's API is read-only, so the app cannot set a lineup. What it can do
//! is say, in slot order, "Shakir over Gainwell in FLEX, +2.1" and "DEF is
//! empty" — which on draft night it did not, and the user finished with an
//! empty defense slot and a running back starting over a better receiver.

use crate::draft::TeamRoster;
use crate::engine::LoadedLeague;
use crate::lineup::{best_lineup, Candidate, Starter};
use crate::roster::RosterRules;
use crate::scoring::norm_cdf;
use crate::sleeper::Matchup;
use serde::Serialize;
use std::collections::HashMap;

/// Week-to-week spread of a fantasy player around his projection, as a
/// fraction of it, by position. Wide, because a weekly projection is a mean
/// over boom and bust games: a quarterback's week is the steadiest, a
/// defense's the wildest.
///
/// Measured, not guessed: every starter projected four points or more in
/// last season's league, 1,746 player-weeks, root mean square of
/// (actual - projected) / projected (`bin/backtest.rs`).
pub fn position_cv(position: &str) -> f64 {
    match position {
        "QB" => 0.44,
        "RB" => 0.57,
        "WR" => 0.63,
        "TE" => 0.62,
        "K" => 0.6,
        "DEF" | "DST" => 0.77,
        _ => 0.6,
    }
}

/// The spread a season of real games wanted, over the one the starters'
/// own spreads add up to. Above 1 because a projection can be wrong about
/// the week in ways a scoring distribution does not cover — a benched back,
/// a game script, an injury in the first quarter — and those upsets land in
/// the tails, where the normal is thin.
///
/// Fitted on last season: 1.3 on the first half of the year cut the log
/// loss of the second half from 0.626 to 0.605, so it is not the fit
/// flattering itself. Held a notch under the 1.5 the whole season asked for,
/// on 98 games of evidence.
pub const SPREAD_CALIBRATION: f64 = 1.3;

/// Two starters on the same NFL team rise and fall together — a quarterback
/// and his receiver most of all. Correlation applied between same-team
/// starters on one side.
const STACK_CORRELATION: f64 = 0.3;

/// player_id -> NFL team, for the stack correlation.
pub type Teams = HashMap<String, String>;

#[derive(Debug, Clone, Serialize)]
pub struct LineupChange {
    pub slot: String,
    /// Who is set there now. `None` for an empty slot.
    pub out: Option<Starter>,
    pub in_: Starter,
    pub gain: f64,
}

#[derive(Debug, Clone, Serialize)]
pub struct LineupCheck {
    /// The lineup as set on Sleeper, scored on this week's projections.
    pub set_points: f64,
    /// The best lineup from the same roster.
    pub best_points: f64,
    /// Slot by slot, what to change. Empty when the set lineup is the best.
    pub changes: Vec<LineupChange>,
    /// Starting slots with nobody in them.
    pub empty_slots: Vec<String>,
    /// Set starters carrying an injury tag that does not sideline them
    /// (Questionable): playing as far as the projection knows, but worth a
    /// look at the inactives before kickoff.
    pub questionable: Vec<Starter>,
}

#[derive(Debug, Clone, Serialize)]
pub struct MatchupPreview {
    pub opponent_slot: u32,
    pub opponent_name: Option<String>,
    /// My best lineup this week.
    pub my_points: f64,
    /// The opponent's lineup as set, or their best if none is set.
    pub opponent_points: f64,
    pub margin: f64,
    /// P(my score > theirs) with both spread `PLAYER_CV` per starter.
    pub win_probability: f64,
    pub my_starters: Vec<Starter>,
    pub opponent_starters: Vec<Starter>,
}

#[derive(Debug, Clone, Serialize)]
pub struct ThisWeek {
    pub week: u32,
    pub lineup: Option<LineupCheck>,
    pub matchup: Option<MatchupPreview>,
}

/// Starting slots in the league's order — what Sleeper's `starters` array
/// lines up against.
fn starting_slots(rules: &RosterRules) -> Vec<&str> {
    rules
        .slots()
        .iter()
        .map(String::as_str)
        .filter(|s| !RosterRules::is_non_starting(s))
        .collect()
}

/// Score a lineup as set: each starter id resolved against the week's
/// candidates, in slot order. Unknown ids (not projected this week) and
/// `"0"` score nothing.
fn set_lineup(
    starters: &[String],
    week: &[Candidate],
    rules: &RosterRules,
) -> Vec<Option<Starter>> {
    starting_slots(rules)
        .iter()
        .enumerate()
        .map(|(i, slot)| {
            let id = starters.get(i)?;
            if id == "0" {
                return None;
            }
            week.iter().find(|c| &c.player_id == id).map(|c| Starter {
                slot: (*slot).to_string(),
                player_id: c.player_id.clone(),
                name: c.name.clone(),
                position: c.position.clone(),
                points: c.points,
                injury: c.injury.clone(),
            })
        })
        .collect()
}

pub fn lineup_check(starters: &[String], week: &[Candidate], rules: &RosterRules) -> LineupCheck {
    let set = set_lineup(starters, week, rules);
    let set_points: f64 = set.iter().flatten().map(|s| s.points).sum();
    let (best_points, best) = best_lineup(week, rules);
    let slots = starting_slots(rules);
    let mut changes = Vec::new();
    let mut empty_slots = Vec::new();
    for (i, slot) in slots.iter().enumerate() {
        let now = set.get(i).cloned().flatten();
        // The best lineup's occupant of this slot index. Slots of one kind
        // are interchangeable, so match by slot name and consume in order.
        let want = best
            .iter()
            .filter(|s| s.slot == *slot)
            .nth(slots[..i].iter().filter(|x| **x == *slot).count());
        let Some(want) = want else {
            // Nobody on the roster can fill it at all — the draft-night
            // case: no defense drafted. Still empty, and the one thing the
            // best lineup cannot say on its own.
            if now.is_none() {
                empty_slots.push((*slot).to_string());
            }
            continue;
        };
        let same = now.as_ref().is_some_and(|n| n.player_id == want.player_id);
        // Also fine if the set lineup starts him elsewhere: a FLEX/WR swap
        // of the same two players is not a change worth reporting.
        let started_somewhere = set.iter().flatten().any(|n| n.player_id == want.player_id);
        if same || started_somewhere {
            continue;
        }
        let gain = want.points - now.as_ref().map_or(0.0, |n| n.points);
        if now.is_none() {
            empty_slots.push((*slot).to_string());
        }
        if gain > 0.05 {
            changes.push(LineupChange {
                slot: (*slot).to_string(),
                out: now,
                in_: want.clone(),
                gain,
            });
        }
    }
    let questionable = set
        .iter()
        .flatten()
        .filter(|s| s.injury.is_some() && s.points > 0.0)
        .cloned()
        .collect();
    LineupCheck {
        set_points,
        best_points,
        changes,
        empty_slots,
        questionable,
    }
}

/// A side's spread: each starter's own, plus the covariance of starters who
/// share an NFL team.
pub fn team_variance(starters: &[Starter], teams: &Teams) -> f64 {
    let sigmas: Vec<f64> = starters
        .iter()
        .map(|s| position_cv(&s.position) * s.points)
        .collect();
    let mut var: f64 = sigmas.iter().map(|x| x * x).sum();
    for i in 0..starters.len() {
        for j in (i + 1)..starters.len() {
            let same = match (
                teams.get(&starters[i].player_id),
                teams.get(&starters[j].player_id),
            ) {
                (Some(a), Some(b)) => a == b,
                _ => false,
            };
            if same {
                var += 2.0 * STACK_CORRELATION * sigmas[i] * sigmas[j];
            }
        }
    }
    var
}

pub fn preview(
    my_week: &[Candidate],
    opponent: (u32, Option<String>, &[String], &[Candidate]),
    rules: &RosterRules,
    teams: &Teams,
) -> MatchupPreview {
    let (opponent_slot, opponent_name, their_set, their_week) = opponent;
    let (my_points, my_starters) = best_lineup(my_week, rules);
    let set: Vec<Starter> = set_lineup(their_set, their_week, rules)
        .into_iter()
        .flatten()
        .collect();
    let (opponent_points, opponent_starters) = if set.is_empty() {
        best_lineup(their_week, rules)
    } else {
        (set.iter().map(|s| s.points).sum(), set)
    };
    let margin = my_points - opponent_points;
    let sigma = SPREAD_CALIBRATION
        * (team_variance(&my_starters, teams) + team_variance(&opponent_starters, teams)).sqrt();
    let win_probability = if sigma > 0.0 {
        norm_cdf(margin / sigma)
    } else if margin > 0.0 {
        1.0
    } else {
        0.5
    };
    MatchupPreview {
        opponent_slot,
        opponent_name,
        my_points,
        opponent_points,
        margin,
        win_probability,
        my_starters,
        opponent_starters,
    }
}

/// The two rosters sharing my matchup id, if the week has pairings.
pub fn opponent_roster_id(matchups: &[Matchup], my_roster_id: u32) -> Option<u32> {
    let mine = matchups.iter().find(|m| m.roster_id == my_roster_id)?;
    let id = mine.matchup_id?;
    matchups
        .iter()
        .find(|m| m.matchup_id == Some(id) && m.roster_id != my_roster_id)
        .map(|m| m.roster_id)
}

/// Lineup check and matchup preview for `my_slot`, from this week's Sleeper
/// matchups. Rosters come from the draft (`rosters`), lineups from Sleeper.
pub fn this_week(
    loaded: &LoadedLeague,
    rosters: &[TeamRoster],
    my_slot: Option<u32>,
    week: u32,
) -> Option<ThisWeek> {
    let my_slot = my_slot?;
    let slot_to_roster: HashMap<u32, u32> = loaded
        .draft
        .slot_to_roster_id
        .as_ref()?
        .iter()
        .filter_map(|(s, r)| s.parse().ok().map(|s: u32| (s, *r)))
        .collect();
    let roster_of = |slot: u32| slot_to_roster.get(&slot).copied();
    let slot_of = |roster_id: u32| {
        slot_to_roster
            .iter()
            .find(|(_, r)| **r == roster_id)
            .map(|(s, _)| *s)
    };
    let my_roster_id = roster_of(my_slot)?;
    let candidates = |slot: u32| {
        rosters.get((slot - 1) as usize).map(|r| {
            let season = crate::lineup::season_candidates(r, &loaded.board, &loaded.board_index);
            crate::lineup::week_candidates(&season, &loaded.weekly_points, week)
        })
    };
    let mine = candidates(my_slot)?;
    let teams: Teams = loaded
        .board
        .iter()
        .filter_map(|p| Some((p.player_id.clone(), p.team.clone()?)))
        .collect();
    let lineup = loaded
        .matchups
        .iter()
        .find(|m| m.roster_id == my_roster_id && !m.starters.is_empty())
        .map(|m| lineup_check(&m.starters, &mine, &loaded.roster_rules));
    let matchup = opponent_roster_id(&loaded.matchups, my_roster_id)
        .and_then(slot_of)
        .and_then(|opp_slot| {
            let theirs = candidates(opp_slot)?;
            let set: &[String] = loaded
                .matchups
                .iter()
                .find(|m| Some(m.roster_id) == roster_of(opp_slot))
                .map_or(&[], |m| m.starters.as_slice());
            let name = rosters
                .get((opp_slot - 1) as usize)
                .and_then(|r| r.display_name.clone());
            Some(preview(
                &mine,
                (opp_slot, name, set, &theirs),
                &loaded.roster_rules,
                &teams,
            ))
        });
    if lineup.is_none() && matchup.is_none() {
        return None;
    }
    Some(ThisWeek {
        week,
        lineup,
        matchup,
    })
}

#[cfg(test)]
mod tests;
