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

/// A player already scored under league rules.
#[derive(Debug, Clone)]
pub struct ScoredPlayer {
    pub position: String,
    pub points: f64,
}

pub struct ReplacementModel {
    /// position -> replacement rank (1-based count of startable players)
    pub demand: HashMap<String, usize>,
    /// position -> replacement-level season points
    pub baseline: HashMap<String, f64>,
}

/// roster_positions comes straight from the Sleeper league
/// (e.g. ["QB","RB","WR","TE","FLEX","FLEX","FLEX","FLEX","DEF","BN",...]).
pub fn flex_eligible(slot: &str) -> Option<Vec<&'static str>> {
    match slot {
        "FLEX" => Some(vec!["RB", "WR", "TE"]),
        "WRRB_FLEX" => Some(vec!["RB", "WR"]),
        "REC_FLEX" => Some(vec!["WR", "TE"]),
        "SUPER_FLEX" => Some(vec!["QB", "RB", "WR", "TE"]),
        _ => None,
    }
}

pub fn compute_replacement(
    players: &[ScoredPlayer],
    roster_positions: &[String],
    teams: usize,
) -> ReplacementModel {
    // League-wide dedicated demand per position.
    let mut base_demand: HashMap<String, usize> = HashMap::new();
    let mut flex_slots: Vec<Vec<&'static str>> = Vec::new();
    for slot in roster_positions {
        if slot == "BN" {
            continue;
        }
        if let Some(elig) = flex_eligible(slot) {
            flex_slots.push(elig);
        } else {
            *base_demand.entry(slot.clone()).or_insert(0) += 1;
        }
    }
    for demand in base_demand.values_mut() {
        *demand *= teams;
    }

    // Sort each position's pool by points, descending.
    let mut pools: HashMap<String, Vec<&ScoredPlayer>> = HashMap::new();
    for p in players {
        pools.entry(p.position.clone()).or_default().push(p);
    }
    for pool in pools.values_mut() {
        pool.sort_by(|a, b| b.points.partial_cmp(&a.points).unwrap_or(std::cmp::Ordering::Equal));
    }

    // Allocate flex demand: pool all players past each position's dedicated
    // starters, take the best (flex_count * teams) of them, and count which
    // positions they come from.
    let mut demand = base_demand.clone();
    if !flex_slots.is_empty() {
        // All current flex slot types in this league share eligibility, so
        // treat them as one pool sized by count. (A league mixing FLEX and
        // REC_FLEX would need per-slot allocation; not needed for v1.)
        let flex_count = flex_slots.len() * teams;
        let eligible: Vec<&str> = flex_slots[0].clone();
        let mut overflow: Vec<&ScoredPlayer> = Vec::new();
        for pos in &eligible {
            let dedicated = base_demand.get(*pos).copied().unwrap_or(0);
            if let Some(pool) = pools.get(*pos) {
                overflow.extend(pool.iter().skip(dedicated).copied());
            }
        }
        overflow.sort_by(|a, b| b.points.partial_cmp(&a.points).unwrap_or(std::cmp::Ordering::Equal));
        for p in overflow.into_iter().take(flex_count) {
            *demand.entry(p.position.clone()).or_insert(0) += 1;
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
        ScoredPlayer { position: pos.into(), points: pts }
    }

    #[test]
    fn flex_demand_goes_to_best_positions() {
        // 2 teams, 1 RB + 1 WR + 1 FLEX each. RBs dominate the overflow, so
        // flex demand should land on RB.
        let roster: Vec<String> = ["RB", "WR", "FLEX", "BN"].iter().map(|s| s.to_string()).collect();
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
        let model = compute_replacement(&players, &roster, 2);
        assert_eq!(model.demand.get("RB"), Some(&4)); // 2 dedicated + 2 flex
        assert_eq!(model.demand.get("WR"), Some(&2)); // dedicated only
    }

    #[test]
    fn tiers_break_on_gaps() {
        let pts = vec![300.0, 295.0, 250.0, 248.0, 246.0, 200.0];
        let tiers = assign_tiers(&pts, 20.0);
        assert_eq!(tiers, vec![1, 1, 2, 2, 2, 3]);
    }
}
