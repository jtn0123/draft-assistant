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

/// Each position's players, best first. Borrowed rather than copied: the
/// allocator only ever reads points.
fn build_pools(players: &[ScoredPlayer]) -> HashMap<String, Vec<&ScoredPlayer>> {
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
    pools
}

/// League-wide starting demand per position: dedicated slots times the number
/// of teams, plus every flex slot handed to whichever eligible position values
/// it most. A flex slot that no eligible pool can still fill is left
/// unallocated rather than guessed at.
fn allocate(
    pools: &HashMap<String, Vec<&ScoredPlayer>>,
    rules: &RosterRules,
    teams: usize,
    bias: f64,
) -> HashMap<String, usize> {
    // One round of that position coming off the board — the span a manager is
    // choosing over when they decide whether to take the flex player now.
    let horizon = teams.max(1);
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

    // Allocate each flex slot independently against its own eligible pool.
    // This remains correct when a league mixes FLEX, REC_FLEX, and SUPER_FLEX.
    let mut demand = base_demand;
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
    demand
}

/// The demand half of the model on its own, for callers that need to know how
/// many of a position the league starts without also needing a replacement
/// level. The recommender's need model reads it: splitting a flex evenly
/// between the positions eligible for it credited a quarterback a quarter of
/// every superflex slot, so the need model and this model disagreed about the
/// same league.
pub fn allocate_demand(
    players: &[ScoredPlayer],
    rules: &RosterRules,
    teams: usize,
    flex_bias: Option<f64>,
) -> HashMap<String, usize> {
    let bias = flex_bias.unwrap_or(DEFAULT_FLEX_BIAS);
    allocate(&build_pools(players), rules, teams, bias)
}

pub fn compute_replacement(
    players: &[ScoredPlayer],
    rules: &RosterRules,
    teams: usize,
    flex_bias: Option<f64>,
) -> ReplacementModel {
    let bias = flex_bias.unwrap_or(DEFAULT_FLEX_BIAS);
    let pools = build_pools(players);
    let demand = allocate(&pools, rules, teams, bias);

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

/// The point gap that starts a new tier, on the board these numbers were
/// fitted on: a standard full-PPR twelve-team league.
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

/// How many players deep a position's starters run in a twelve-team league,
/// and the median season points of that group under standard full PPR.
///
/// This is the ruler the constants above were measured with. Without it the
/// gap thresholds are absolute season points applied to whatever scale the
/// league happens to use: a six-point-per-passing-touchdown, 0.5-per-carry
/// house league scores roughly double, so every real gap cleared 12 points and
/// the board came back as one long chain of one-man tiers. A tiny-scale league
/// (best-ball fractions, a two-week playoff pool) does the opposite and hands
/// back a single tier per position.
fn reference_level(position: &str) -> (usize, f64) {
    match position {
        "QB" => (24, 300.0),
        "RB" => (36, 180.0),
        "WR" => (36, 185.0),
        "TE" => (12, 140.0),
        "K" => (12, 130.0),
        "DEF" => (12, 110.0),
        _ => (36, 180.0),
    }
}

/// This league's point scale at `position`, as a multiple of the full-PPR
/// board the tier constants were fitted on. Clamped, because a pool that is
/// mostly zeroes (a projection source that failed halfway) must not collapse
/// every position into one tier or blow it apart into forty.
fn point_scale(position: &str, sorted_points: &[f64]) -> f64 {
    let (depth, reference) = reference_level(position);
    // Four players is not a scale. Below that the league's own numbers say
    // less than the default does.
    if sorted_points.len() < 4 || reference <= 0.0 {
        return 1.0;
    }
    let top = &sorted_points[..depth.min(sorted_points.len())];
    let median = top[top.len() / 2];
    if median <= 0.0 {
        return 1.0;
    }
    (median / reference).clamp(0.25, 4.0)
}

/// The gap that starts a new tier at `position`, put on this league's own
/// point scale. `sorted_points` is that position's pool, best first.
pub fn tier_gap_threshold_for(position: &str, sorted_points: &[f64]) -> f64 {
    tier_gap_threshold(position) * point_scale(position, sorted_points)
}

#[cfg(test)]
#[path = "valuation_tests.rs"]
mod tests;
