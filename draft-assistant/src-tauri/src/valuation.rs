//! Replacement levels, VORP, and tiers.
//!
//! Demand model (borrowed from fantasy-bot's README, fixed per its own
//! failure notes): each starting slot contributes demand to the positions
//! eligible for it; dedicated slots contribute 1, flex slots are allocated to
//! the eligible positions that actually want them (not split evenly). Bench
//! does NOT inflate replacement level. Replacement value at a position = mean
//! of the 3 players around the replacement rank, to smooth cliffs.
//!
//! Flex allocation used to go to whichever eligible position had the best
//! *raw points* at its current demand index, and that quietly handed the
//! league to wide receivers: the WR pool is both deeper and flatter, so once
//! it was ahead on raw points it stayed ahead for every remaining slot. A
//! 12-team 2×FLEX league came out at WR 40 / RB 32 — 16 of the 24 flex slots
//! to WR — and every simulated roster ended 6-7 WR deep while the RB-scarcity
//! guard never fired.
//!
//! What a flex slot is actually worth to a position is not the level of the
//! next player but how much is lost by going one round deeper there, so each
//! slot now goes to the best `points + FLEX_BIAS * (points - points one round
//! deeper)`. The scarcity premium is what separates a cliff from a plateau;
//! the level term is still there so a shallow position (TE) cannot win a flex
//! slot on steepness alone at a point where its players are plainly worse.
//! `flex_bias = 0.0` reproduces the old raw-points allocator exactly.

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

/// How heavily a position's own scarcity counts when a flex slot is handed
/// out, relative to the raw points of the next player it would start.
///
/// Tuned on the real 12-team 2×FLEX league (Sleeper 1400180817463881728,
/// 15 rounds): 0.0 gives the old RB 32 / WR 40, 0.25 gives RB 35 / WR 37 —
/// which is where conventional practice puts a 12-team replacement level —
/// and anything past ~0.75 starts pulling flex slots onto tight ends.
pub const DEFAULT_FLEX_BIAS: f64 = 0.25;

/// The score a flex slot sees for `position` at its current demand index: the
/// next player's points, plus `bias` times what that position loses by going
/// one full round deeper. A pool that runs out short of the horizon is
/// measured against its own last player, which is the right answer — the drop
/// really is that far.
fn flex_score(pool: &[&ScoredPlayer], index: usize, horizon: usize, bias: f64) -> Option<f64> {
    let next = pool.get(index)?.points;
    let deeper = pool[(index + horizon).min(pool.len() - 1)].points;
    Some(next + bias * (next - deeper))
}

pub fn compute_replacement(
    players: &[ScoredPlayer],
    rules: &RosterRules,
    teams: usize,
    flex_bias: Option<f64>,
) -> ReplacementModel {
    let bias = flex_bias.unwrap_or(DEFAULT_FLEX_BIAS);
    // One round of that position coming off the board — the span a manager is
    // choosing over when they decide whether to take the flex player now.
    let horizon = teams.max(1);
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
                    let pool = pools.get(*position)?;
                    Some((*position, flex_score(pool, index, horizon, bias)?))
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
#[path = "valuation_tests.rs"]
mod tests;
