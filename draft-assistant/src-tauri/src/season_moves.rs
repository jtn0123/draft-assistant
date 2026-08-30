//! Waiver targets: which free agents would actually change your lineup.
//!
//! A waiver target is worth claiming only if it would actually change the
//! lineup you start, so "gain" here is measured the honest way: re-solve the
//! optimal lineup with the free agent added and see how much the total moves.
//! A high-scoring player who still would not crack your starting eleven scores
//! a gain of zero, which is correct.

use crate::roster::RosterRules;
use crate::season_lineup::{optimal_lineup, Candidate};
use serde::Serialize;
use std::collections::HashSet;

/// How many free agents to evaluate. They arrive ranked by projection, so the
/// tail cannot displace a starter and is not worth the lineup solves.
const CANDIDATE_POOL: usize = 60;
const MAX_TARGETS: usize = 6;

#[derive(Debug, Clone, Serialize)]
pub struct WaiverTarget {
    pub player_id: String,
    pub name: String,
    pub position: String,
    pub team: Option<String>,
    /// Points added to a typical week's starting lineup.
    pub gain_points: f64,
    /// Same gain as a fraction of the current lineup total, 0.0..
    pub gain_fraction: f64,
    /// Suggested FAAB bid, or `None` in a league without a budget.
    pub suggested_bid: Option<u32>,
    /// Other teams this player would also improve.
    pub rivals: usize,
}

/// A roster we might be bidding against.
pub struct RivalRoster<'a> {
    pub roster_id: u32,
    pub player_ids: &'a [String],
}

/// Free agent under consideration, already scored for a typical week.
#[derive(Debug, Clone)]
pub struct FreeAgent {
    pub player_id: String,
    pub name: String,
    pub position: String,
    pub team: Option<String>,
    pub weekly_points: f64,
}

fn lineup_total(rules: &RosterRules, candidates: &[Candidate]) -> f64 {
    optimal_lineup(rules, candidates)
        .iter()
        .map(|s| s.points)
        .sum()
}

/// Marginal value of adding one player to a roster's best lineup, given that
/// roster's baseline total. The caller passes the baseline because it is the
/// same for every candidate considered against the same roster — recomputing
/// it per candidate doubled the number of lineup solves.
fn marginal_gain(
    rules: &RosterRules,
    base: &[Candidate],
    baseline: f64,
    addition: &Candidate,
) -> f64 {
    let mut with = base.to_vec();
    with.push(addition.clone());
    (lineup_total(rules, &with) - baseline).max(0.0)
}

/// Rank free agents by how much they would improve my starting lineup.
///
/// `budget_left` is remaining FAAB; the suggested bid scales with how much of
/// my lineup the player actually fixes, so a marginal streamer never eats the
/// whole budget.
#[allow(clippy::too_many_arguments)]
pub fn waiver_targets(
    rules: &RosterRules,
    my_candidates: &[Candidate],
    free_agents: &[FreeAgent],
    rivals: &[RivalRoster],
    rival_candidates: &impl Fn(&[String]) -> Vec<Candidate>,
    budget_left: Option<f64>,
) -> Vec<WaiverTarget> {
    let baseline = lineup_total(rules, my_candidates);
    // Each rival's roster and baseline are identical for every free agent we
    // consider, so build them once instead of once per (agent, rival) pair —
    // that was 60 x 13 reconstructions per refresh.
    let rival_pools: Vec<(Vec<Candidate>, f64)> = rivals
        .iter()
        .map(|r| {
            let candidates = rival_candidates(r.player_ids);
            let total = lineup_total(rules, &candidates);
            (candidates, total)
        })
        .collect();
    let mut scored: Vec<WaiverTarget> = free_agents
        .iter()
        .take(CANDIDATE_POOL)
        .filter_map(|fa| {
            let addition = Candidate {
                player_id: fa.player_id.clone(),
                position: fa.position.clone(),
                points: fa.weekly_points,
            };
            let gain = marginal_gain(rules, my_candidates, baseline, &addition);
            if gain <= 0.05 {
                return None;
            }
            let rival_count = rival_pools
                .iter()
                .filter(|(candidates, total)| {
                    marginal_gain(rules, candidates, *total, &addition) > 0.05
                })
                .count();
            let fraction = if baseline > 0.0 { gain / baseline } else { 0.0 };
            Some(WaiverTarget {
                player_id: fa.player_id.clone(),
                name: fa.name.clone(),
                position: fa.position.clone(),
                team: fa.team.clone(),
                gain_points: gain,
                gain_fraction: fraction,
                suggested_bid: budget_left.map(|budget| suggest_bid(budget, fraction, rival_count)),
                rivals: rival_count,
            })
        })
        .collect();
    scored.sort_by(|a, b| b.gain_points.total_cmp(&a.gain_points));
    scored.truncate(MAX_TARGETS);
    scored
}

/// Bid a share of the remaining budget proportional to the lineup improvement,
/// nudged up when other teams want the same player. Capped well short of the
/// full budget: no single in-season add is worth everything you have left.
fn suggest_bid(budget_left: f64, gain_fraction: f64, rivals: usize) -> u32 {
    if budget_left <= 0.0 {
        return 0;
    }
    let competition = 1.0 + (rivals as f64 * 0.25).min(1.0);
    let share = (gain_fraction * 2.5 * competition).clamp(0.01, 0.5);
    (budget_left * share).round().max(1.0) as u32
}

/// Player ids rostered anywhere in the league — everyone else is a free agent.
pub fn rostered_ids<'a>(rosters: impl IntoIterator<Item = &'a [String]>) -> HashSet<String> {
    rosters
        .into_iter()
        .flat_map(|ids| ids.iter().cloned())
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;

    fn rules(slots: &[&str]) -> RosterRules {
        RosterRules::new(&slots.iter().map(|s| (*s).to_string()).collect::<Vec<_>>())
    }

    fn candidate(id: &str, position: &str, points: f64) -> Candidate {
        Candidate {
            player_id: id.into(),
            position: position.into(),
            points,
        }
    }

    fn agent(id: &str, position: &str, points: f64) -> FreeAgent {
        FreeAgent {
            player_id: id.into(),
            name: id.to_uppercase(),
            position: position.into(),
            team: Some("JAX".into()),
            weekly_points: points,
        }
    }

    #[test]
    fn a_free_agent_who_cannot_crack_the_lineup_is_not_a_target() {
        let rules = rules(&["RB", "BN"]);
        let mine = vec![candidate("rb1", "RB", 20.0)];
        let targets = waiver_targets(
            &rules,
            &mine,
            &[agent("scrub", "RB", 4.0)],
            &[],
            &|_: &[String]| Vec::new(),
            Some(100.0),
        );
        assert!(targets.is_empty());
    }

    #[test]
    fn gain_is_the_upgrade_over_the_displaced_starter_not_raw_points() {
        let rules = rules(&["RB", "BN"]);
        let mine = vec![candidate("rb1", "RB", 12.0)];
        let targets = waiver_targets(
            &rules,
            &mine,
            &[agent("stud", "RB", 18.0)],
            &[],
            &|_: &[String]| Vec::new(),
            Some(100.0),
        );
        assert_eq!(targets.len(), 1);
        assert!((targets[0].gain_points - 6.0).abs() < 1e-9);
        assert!((targets[0].gain_fraction - 0.5).abs() < 1e-9);
    }

    #[test]
    fn rivals_count_teams_the_player_would_also_help() {
        let rules = rules(&["RB", "BN"]);
        let mine = vec![candidate("rb1", "RB", 12.0)];
        let rivals = vec![
            RivalRoster {
                roster_id: 2,
                player_ids: &[],
            },
            RivalRoster {
                roster_id: 3,
                player_ids: &[],
            },
        ];
        // Roster 2 is weak at RB and would improve; roster 3 is stacked.
        let targets = waiver_targets(
            &rules,
            &mine,
            &[agent("stud", "RB", 18.0)],
            &rivals,
            &|ids: &[String]| {
                if ids.is_empty() {
                    vec![candidate("weak", "RB", 5.0)]
                } else {
                    vec![candidate("strong", "RB", 30.0)]
                }
            },
            Some(100.0),
        );
        assert_eq!(targets[0].rivals, 2);
    }

    #[test]
    fn bids_scale_with_the_upgrade_and_never_eat_the_budget() {
        assert_eq!(suggest_bid(0.0, 0.5, 0), 0);
        let small = suggest_bid(100.0, 0.02, 0);
        let large = suggest_bid(100.0, 0.30, 0);
        assert!(small >= 1 && small < large);
        assert!(large <= 50, "bid {large} exceeded half the budget");
    }

    #[test]
    fn leagues_without_faab_get_no_suggested_bid() {
        let rules = rules(&["RB", "BN"]);
        let targets = waiver_targets(
            &rules,
            &[candidate("rb1", "RB", 12.0)],
            &[agent("stud", "RB", 18.0)],
            &[],
            &|_: &[String]| Vec::new(),
            None,
        );
        assert_eq!(targets[0].suggested_bid, None);
    }

    #[test]
    fn rostered_ids_unions_every_roster() {
        let a = vec!["1".to_string(), "2".to_string()];
        let b = vec!["2".to_string(), "3".to_string()];
        let ids = rostered_ids([a.as_slice(), b.as_slice()]);
        assert_eq!(ids.len(), 3);
    }
}
