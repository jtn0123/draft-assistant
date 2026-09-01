//! Waiver targets: which free agents would actually change your lineup.
//!
//! A waiver target is worth claiming only if it would actually change the
//! lineup you start, so "gain" here is measured the honest way: re-solve the
//! optimal lineup with the free agent added and see how much the total moves.
//! A high-scoring player who still would not crack your starting eleven scores
//! a gain of zero, which is correct.

use crate::roster::RosterRules;
use crate::season_lineup::{lineup_total, Candidate};
use serde::Serialize;
use std::collections::HashSet;

/// How many free agents to evaluate. The caller ranks them by this week's
/// projection and cuts the pool to this size, so the tail cannot displace a
/// starter and is not worth the lineup solves. This is the one place the size
/// is defined; the `take` below is only a guard for callers that pass more.
pub const CANDIDATE_POOL: usize = 60;
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

/// A roster set up for repeated what-if tests: its candidates plus one spare
/// slot at the end, and the total its best lineup puts up as it stands.
///
/// The baseline is computed once because it is the same for every candidate
/// tested against this roster, and the roster itself is copied once, when the
/// pool is built. Rebuilding it per candidate — a fresh `Vec<Candidate>`, two
/// `String`s apiece — cost a full roster clone for every (free agent, roster)
/// pair, which at sixty agents against a twelve-team league is some seven
/// hundred of them per rebuild. `season_trades` already worked this way; this
/// call site was simply never converted.
struct Pool {
    /// The roster, with a spare slot at the end that each candidate in turn is
    /// written into. Its contents are always overwritten before being read.
    scratch: Vec<Candidate>,
    baseline: f64,
}

impl Pool {
    fn new(rules: &RosterRules, candidates: Vec<Candidate>) -> Self {
        let baseline = lineup_total(rules, &candidates);
        let mut scratch = candidates;
        scratch.push(Candidate {
            player_id: String::new(),
            position: String::new(),
            points: 0.0,
        });
        Self { scratch, baseline }
    }

    /// How much this roster's best lineup improves with `addition` in it.
    fn gain_from(&mut self, rules: &RosterRules, addition: &Candidate) -> f64 {
        if let Some(slot) = self.scratch.last_mut() {
            slot.clone_from(addition);
        }
        (lineup_total(rules, &self.scratch) - self.baseline).max(0.0)
    }
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
    let mut mine = Pool::new(rules, my_candidates.to_vec());
    let baseline = mine.baseline;
    // Each rival's roster and baseline are identical for every free agent we
    // consider, so build them once instead of once per (agent, rival) pair.
    let mut rival_pools: Vec<Pool> = rivals
        .iter()
        .map(|r| Pool::new(rules, rival_candidates(r.player_ids)))
        .collect();
    let mut addition = Candidate {
        player_id: String::new(),
        position: String::new(),
        points: 0.0,
    };
    let mut scored: Vec<WaiverTarget> = Vec::new();
    for fa in free_agents.iter().take(CANDIDATE_POOL) {
        addition.player_id.clone_from(&fa.player_id);
        addition.position.clone_from(&fa.position);
        addition.points = fa.weekly_points;

        let gain = mine.gain_from(rules, &addition);
        if gain <= 0.05 {
            continue;
        }
        let mut rival_count = 0;
        for pool in &mut rival_pools {
            if pool.gain_from(rules, &addition) > 0.05 {
                rival_count += 1;
            }
        }
        let fraction = if baseline > 0.0 { gain / baseline } else { 0.0 };
        scored.push(WaiverTarget {
            player_id: fa.player_id.clone(),
            name: fa.name.clone(),
            position: fa.position.clone(),
            team: fa.team.clone(),
            gain_points: gain,
            gain_fraction: fraction,
            suggested_bid: budget_left.map(|budget| suggest_bid(budget, fraction, rival_count)),
            rivals: rival_count,
        });
    }
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
