//! Replacement levels, VORP, and tiers.
//!
//! Demand model (borrowed from fantasy-bot's README, fixed per its own
//! failure notes): each starting slot contributes demand to the positions
//! eligible for it; dedicated slots contribute 1, flex slots are allocated to
//! whichever eligible positions actually hold the best remaining projected
//! players (not split evenly). Bench does NOT inflate replacement level.
//! Replacement value at a position = mean of the 3 players around the
//! replacement rank, to smooth cliffs.

use std::collections::HashMap;

use crate::roster::RosterRules;

/// A player already scored under league rules.
#[derive(Debug, Clone)]
pub struct ScoredPlayer {
    pub position: String,
    pub points: f64,
}

#[derive(Debug, Clone, Default)]
pub struct ReplacementModel {
    /// position -> replacement rank (1-based count of startable players)
    pub demand: HashMap<String, usize>,
    /// position -> replacement-level season points
    pub baseline: HashMap<String, f64>,
}

pub fn compute_replacement(
    players: &[ScoredPlayer],
    rules: &RosterRules,
    teams: usize,
) -> ReplacementModel {
    // League-wide dedicated demand per position.
    let mut base_demand: HashMap<String, usize> = HashMap::new();
    let mut flex_slots: Vec<&[&str]> = Vec::new();
    for slot in rules.slots() {
        if RosterRules::is_non_starting(slot) {
            continue;
        }
        if let Some(elig) = RosterRules::flex_eligible(slot) {
            flex_slots.push(elig);
        } else {
            *base_demand.entry(slot.clone()).or_insert(0) += 1;
        }
    }
    for demand in base_demand.values_mut() {
        *demand *= teams;
    }
    flex_slots.sort_by_key(|eligible| eligible.len());

    // Sort each position's pool by points, descending.
    let mut pools: HashMap<String, Vec<&ScoredPlayer>> = HashMap::new();
    for p in players {
        pools.entry(p.position.clone()).or_default().push(p);
    }
    for pool in pools.values_mut() {
        pool.sort_by(|a, b| {
            b.points
                .partial_cmp(&a.points)
                .unwrap_or(std::cmp::Ordering::Equal)
        });
    }

    // Allocate each flex slot independently against its own eligible pool.
    // This remains correct when a league mixes FLEX, REC_FLEX, and SUPER_FLEX.
    let mut demand = base_demand.clone();
    for eligible in flex_slots {
        for _ in 0..teams {
            let best_position = eligible
                .iter()
                .filter_map(|position| {
                    let index = demand.get(*position).copied().unwrap_or(0);
                    pools
                        .get(*position)
                        .and_then(|pool| pool.get(index))
                        .map(|player| (*position, player.points))
                })
                .max_by(|a, b| a.1.partial_cmp(&b.1).unwrap_or(std::cmp::Ordering::Equal))
                .map(|(position, _)| position);
            if let Some(position) = best_position {
                *demand.entry(position.to_string()).or_insert(0) += 1;
            }
        }
    }

    // Baseline = mean points of the 3 players bracketing the replacement rank.
    let mut baseline: HashMap<String, f64> = HashMap::new();
    for (pos, rank) in &demand {
        let Some(pool) = pools.get(pos) else { continue };
        if pool.is_empty() {
            continue;
        }
        let idx = (*rank).min(pool.len().saturating_sub(1));
        let lo = idx.saturating_sub(1);
        let hi = (idx + 2).min(pool.len());
        let window = &pool[lo..hi];
        let mean = window.iter().map(|p| p.points).sum::<f64>() / window.len() as f64;
        baseline.insert(pos.clone(), mean);
    }

    ReplacementModel { demand, baseline }
}

/// Assign tiers within a position by point-gap clustering: a new tier starts
/// wherever the drop from the previous player exceeds the threshold.
pub fn assign_tiers(sorted_points: &[f64], gap_threshold: f64) -> Vec<u32> {
    let mut tiers = Vec::with_capacity(sorted_points.len());
    let mut tier = 1u32;
    for (i, pts) in sorted_points.iter().enumerate() {
        if i > 0 {
            let gap = sorted_points[i - 1] - pts;
            if gap > gap_threshold {
                tier += 1;
            }
        }
        tiers.push(tier);
    }
    tiers
}

pub fn tier_gap_threshold(position: &str) -> f64 {
    match position {
        "QB" => 14.0,
        "RB" => 12.0,
        "WR" => 12.0,
        "TE" => 12.0,
        "DEF" => 8.0,
        _ => 12.0,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn sp(pos: &str, pts: f64) -> ScoredPlayer {
        ScoredPlayer {
            position: pos.into(),
            points: pts,
        }
    }

    #[test]
    fn flex_demand_goes_to_best_positions() {
        // 2 teams, 1 RB + 1 WR + 1 FLEX each. RBs dominate the overflow, so
        // flex demand should land on RB.
        let roster: Vec<String> = ["RB", "WR", "FLEX", "BN"]
            .iter()
            .map(|s| s.to_string())
            .collect();
        let players = vec![
            sp("RB", 300.0),
            sp("RB", 290.0),
            sp("RB", 280.0),
            sp("RB", 270.0),
            sp("WR", 250.0),
            sp("WR", 240.0),
            sp("WR", 100.0),
            sp("WR", 90.0),
        ];
        let model = compute_replacement(&players, &RosterRules::new(&roster), 2);
        assert_eq!(model.demand.get("RB"), Some(&4)); // 2 dedicated + 2 flex
        assert_eq!(model.demand.get("WR"), Some(&2)); // dedicated only
    }

    #[test]
    fn mixed_flex_types_allocate_only_eligible_players() {
        let players = vec![
            sp("QB", 300.0),
            sp("QB", 290.0),
            sp("QB", 280.0),
            sp("WR", 250.0),
            sp("WR", 240.0),
            sp("WR", 230.0),
            sp("TE", 220.0),
            sp("TE", 210.0),
            sp("RB", 200.0),
            sp("RB", 190.0),
        ];
        let slots = ["QB", "SUPER_FLEX", "REC_FLEX"]
            .iter()
            .map(|slot| (*slot).to_string())
            .collect::<Vec<_>>();
        let model = compute_replacement(&players, &RosterRules::new(&slots), 1);

        assert_eq!(model.demand.get("QB"), Some(&2));
        assert_eq!(model.demand.get("WR"), Some(&1));
    }

    #[test]
    fn tiers_break_on_gaps() {
        let pts = vec![300.0, 295.0, 250.0, 248.0, 246.0, 200.0];
        let tiers = assign_tiers(&pts, 20.0);
        assert_eq!(tiers, vec![1, 1, 2, 2, 2, 3]);
    }
}
