//! Optimal lineups, and the gap between the optimal one and what is actually set.
//!
//! "Calls to make" in the season screen is exactly this diff: fill every
//! starting slot with the best eligible player, then report where that
//! disagrees with the roster's current starters.

use crate::roster::RosterRules;
use crate::season_spread::{self, Starter};
use crate::weekly::WeeklyPoints;
use serde::Serialize;
use std::collections::HashSet;

/// One filled starting slot.
#[derive(Debug, Clone, Serialize)]
pub struct LineupSlot {
    /// Roster slot label: "QB", "RB", "FLEX", …
    pub slot: String,
    pub player_id: Option<String>,
    pub points: f64,
}

/// A player considered for a lineup slot.
#[derive(Debug, Clone)]
pub struct Candidate {
    pub player_id: String,
    pub position: String,
    pub points: f64,
}

/// Fill the roster's starting slots with the highest-scoring eligible players.
///
/// Slots are filled most-constrained first (a plain "TE" before a "REC_FLEX"
/// before a "SUPER_FLEX"), which is what makes the greedy pass optimal here:
/// every slot's eligibility set is either disjoint from or a superset of the
/// ones filled before it, so taking the best available for a narrow slot can
/// never strand a wider slot with nothing to put in it.
pub fn optimal_lineup(rules: &RosterRules, candidates: &[Candidate]) -> Vec<LineupSlot> {
    let mut order: Vec<(usize, &String)> = rules
        .slots()
        .iter()
        .enumerate()
        .filter(|(_, slot)| !RosterRules::is_non_starting(slot))
        .collect();
    order.sort_by_key(|(index, slot)| {
        let width = RosterRules::flex_eligible(slot).map_or(1, <[&str]>::len);
        (width, *index)
    });

    let mut used: HashSet<&str> = HashSet::new();
    let mut filled: Vec<(usize, LineupSlot)> = Vec::new();
    for (index, slot) in order {
        let best = candidates
            .iter()
            .filter(|c| !used.contains(c.player_id.as_str()))
            .filter(|c| RosterRules::can_fill(slot, &c.position))
            .max_by(|a, b| a.points.total_cmp(&b.points));
        match best {
            Some(c) => {
                used.insert(c.player_id.as_str());
                filled.push((
                    index,
                    LineupSlot {
                        slot: slot.clone(),
                        player_id: Some(c.player_id.clone()),
                        points: c.points,
                    },
                ));
            }
            None => filled.push((
                index,
                LineupSlot {
                    slot: slot.clone(),
                    player_id: None,
                    points: 0.0,
                },
            )),
        }
    }
    // Back to the league's own slot order for display.
    filled.sort_by_key(|(index, _)| *index);
    filled.into_iter().map(|(_, slot)| slot).collect()
}

/// One remaining week of a roster's outlook: the best lineup it can field and
/// how far that score is expected to wander from its own projection.
#[derive(Debug, Clone, Copy)]
pub struct WeekOutlook {
    pub week: u32,
    pub points: f64,
    /// Calibrated standard deviation of `points`, from the starters that made
    /// the lineup — see [`crate::season_spread`].
    pub sigma: f64,
}

/// The best lineup this roster can field, week by week, with its spread.
///
/// A player's position does not change from one week to the next, so the
/// candidate list is built once per roster and only the points are rewritten
/// for each week. Built per (roster, week) instead — which is what the
/// standings and the Trends snapshot both used to do — twelve rosters over
/// fourteen weeks cost thousands of dictionary lookups and twice as many
/// throwaway `String`s on every rebuild.
pub fn weekly_lineup_outlook(
    rules: &RosterRules,
    player_ids: &[String],
    position_of: &impl Fn(&str) -> Option<String>,
    team_of: &impl Fn(&str) -> Option<String>,
    sidelined: &impl Fn(&str) -> bool,
    weekly: &WeeklyPoints,
    weeks: impl IntoIterator<Item = u32>,
) -> Vec<WeekOutlook> {
    // Week 0 scores nothing; every entry is overwritten below before it is read.
    let mut candidates = candidates_for(player_ids, position_of, sidelined, weekly, 0);
    // Whether each candidate is sidelined is settled once, alongside his
    // position: the dictionary does not change between the weeks of one
    // rebuild, and asking it again per (player, week) is the lookup storm the
    // single candidate list exists to avoid.
    let benched: Vec<bool> = candidates.iter().map(|c| sidelined(&c.player_id)).collect();
    weeks
        .into_iter()
        .map(|week| {
            for (candidate, out) in candidates.iter_mut().zip(&benched) {
                candidate.points = if *out {
                    0.0
                } else {
                    weekly.get_or_zero(&candidate.player_id, week)
                };
            }
            let lineup = optimal_lineup(rules, &candidates);
            let starters: Vec<Starter> = season_spread::starters_of(&lineup, position_of, team_of);
            WeekOutlook {
                week,
                points: lineup.iter().map(|s| s.points).sum(),
                sigma: season_spread::team_sigma(&starters),
            }
        })
        .collect()
}

/// Just the totals from [`weekly_lineup_outlook`], for callers that do not
/// price risk.
pub fn weekly_lineup_totals(
    rules: &RosterRules,
    player_ids: &[String],
    position_of: &impl Fn(&str) -> Option<String>,
    team_of: &impl Fn(&str) -> Option<String>,
    sidelined: &impl Fn(&str) -> bool,
    weekly: &WeeklyPoints,
    weeks: impl IntoIterator<Item = u32>,
) -> Vec<(u32, f64)> {
    weekly_lineup_outlook(
        rules,
        player_ids,
        position_of,
        team_of,
        sidelined,
        weekly,
        weeks,
    )
    .into_iter()
    .map(|w| (w.week, w.points))
    .collect()
}

/// Build week-scored candidates for every player on a roster. Players with no
/// projection this week (bye, or unprojected) are still candidates at zero, so
/// a roster that is short at a position still reports the slot as filled-empty
/// rather than silently dropping it.
///
/// A player `sidelined` says is Out or Doubtful is kept on the list and scored
/// at zero rather than dropped. Sleeper leaves his weekly projection standing
/// long after the injury report lands, and taking it at face value put a
/// player who will not take the field into the optimal lineup — inflating both
/// sides of the matchup, and the win probability with them.
pub fn candidates_for(
    player_ids: &[String],
    position_of: &impl Fn(&str) -> Option<String>,
    sidelined: &impl Fn(&str) -> bool,
    weekly: &WeeklyPoints,
    week: u32,
) -> Vec<Candidate> {
    player_ids
        .iter()
        .filter_map(|id| {
            let position = position_of(id)?;
            Some(Candidate {
                player_id: id.clone(),
                position,
                points: if sidelined(id) {
                    0.0
                } else {
                    weekly.get_or_zero(id, week)
                },
            })
        })
        .collect()
}

/// A start/sit call: swap `out` for `in` at `slot` and gain `gain` points.
#[derive(Debug, Clone, Serialize)]
pub struct LineupCall {
    pub slot: String,
    pub player_in: String,
    pub player_in_id: String,
    pub player_in_team: Option<String>,
    pub player_out: String,
    pub player_out_id: String,
    pub gain: f64,
    pub why: String,
    /// One line of plain language for *why*, beyond the point difference:
    /// "he's on bye", "your starter is listed Out". Filled in by
    /// `season_calls`, which is where the injury and schedule data lives.
    #[serde(default)]
    pub reason: Option<String>,
    /// Epoch milliseconds by which the swap has to be made — the earlier of
    /// the two players' kickoffs. `None` when the scoreboard has no game for
    /// either of them, or when both have already started.
    #[serde(default)]
    pub locks_at_ms: Option<i64>,
}

/// Diff the optimal lineup against the starters actually set.
///
/// A call is "start X over Y". X is anyone in the optimal lineup who is not
/// starting at all; Y is a set starter who is not in the optimal lineup (or an
/// empty slot). Players who are optimal but already starting in a *different*
/// slot are never a call — moving someone between two slots they both qualify
/// for changes nothing — and, crucially, they never block a call either: if
/// the set lineup is a shuffled copy of the optimal one apart from a single
/// bench-worthy starter, that starter is still reported.
///
/// Incoming players are matched, best first, to the lowest-scoring outgoing
/// starter sitting in a slot they are eligible for (`eligible(slot, id)`), so
/// the slot named on the call is one the user can actually put them in.
pub fn calls_from_diff(
    optimal: &[LineupSlot],
    current: &[LineupSlot],
    eligible: &impl Fn(&str, &str) -> bool,
    describe: &impl Fn(&str) -> (String, Option<String>),
    reason: &impl Fn(&str, &str, &str) -> String,
) -> Vec<LineupCall> {
    let optimal_ids: HashSet<&str> = optimal
        .iter()
        .filter_map(|s| s.player_id.as_deref())
        .collect();
    let current_ids: HashSet<&str> = current
        .iter()
        .filter_map(|s| s.player_id.as_deref())
        .collect();

    let mut incoming: Vec<&LineupSlot> = optimal
        .iter()
        .filter(|s| {
            s.player_id
                .as_deref()
                .is_some_and(|id| !current_ids.contains(id))
        })
        .collect();
    incoming.sort_by(|a, b| b.points.total_cmp(&a.points));

    // Set starters who should not be starting, plus every empty set slot.
    let mut outgoing: Vec<&LineupSlot> = current
        .iter()
        .filter(|s| {
            s.player_id
                .as_deref()
                .is_none_or(|id| !optimal_ids.contains(id))
        })
        .collect();

    let mut calls = Vec::new();
    for best in incoming {
        let Some(best_id) = best.player_id.as_deref() else {
            continue;
        };
        let pick = |fits: &dyn Fn(&LineupSlot) -> bool| {
            outgoing
                .iter()
                .enumerate()
                .filter(|(_, out)| fits(out))
                .min_by(|(_, a), (_, b)| a.points.total_cmp(&b.points))
                .map(|(i, _)| i)
        };
        let Some(index) = pick(&|out| eligible(&out.slot, best_id)).or_else(|| pick(&|_| true))
        else {
            break;
        };
        let now = outgoing.remove(index);
        let now_id = now.player_id.as_deref().unwrap_or("");
        let gain = best.points - now.points;
        if gain <= 0.05 {
            continue;
        }
        let (in_name, in_team) = describe(best_id);
        let (out_name, _) = if now_id.is_empty() {
            ("an empty slot".to_string(), None)
        } else {
            describe(now_id)
        };
        calls.push(LineupCall {
            slot: now.slot.clone(),
            player_in: in_name,
            player_in_id: best_id.to_string(),
            player_in_team: in_team,
            player_out: out_name,
            player_out_id: now_id.to_string(),
            gain,
            why: reason(&now.slot, best_id, now_id),
            reason: None,
            locks_at_ms: None,
        });
    }
    calls.sort_by(|a, b| b.gain.total_cmp(&a.gain));
    calls
}

#[cfg(test)]
mod tests;
