//! Demand model: each starting slot contributes demand to the positions
//! eligible for it; dedicated slots contribute 1; flex slots go to whichever
//! eligible position the *market* starts next (the candidate with the
//! earliest ADP), falling back to projected points when nobody has one.
//! Bench does NOT inflate replacement level. Replacement value at a position
//! = mean of the 3 players around the replacement rank, to smooth cliffs.
//!
//! Why the market and not our own projections: allocating flex to the best
//! projected player was circular. In full PPR receivers project higher, so
//! the model decided the league starts 52 receivers and 31 backs, set the RB
//! baseline at RB31 (about 157 points, the same as WR) and told the user that
//! waiting on running backs was free. The room disagreed: by pick 132 it had
//! taken 45 backs, and the one actually available was 133 points. Allocating
//! by ADP on the same pool gives RB41/WR42 and baselines of 123 and 170,
//! within ten points of what was really left on the board at the end.
use std::collections::HashMap;

use crate::roster::RosterRules;

/// A player already scored under league rules.
#[derive(Debug, Clone)]
pub struct ScoredPlayer {
    pub position: String,
    pub points: f64,
    /// Market draft position, if known. Decides where flex demand goes.
    pub adp: Option<f64>,
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
    // Each slot goes to the position whose next man up the market drafts
    // soonest; only when no candidate carries an ADP does projection decide.
    let mut demand = base_demand.clone();
    for eligible in flex_slots {
        for _ in 0..teams {
            let candidates: Vec<(&str, &ScoredPlayer)> = eligible
                .iter()
                .filter_map(|position| {
                    let index = demand.get(*position).copied().unwrap_or(0);
                    pools
                        .get(*position)
                        .and_then(|pool| pool.get(index))
                        .map(|player| (*position, *player))
                })
                .collect();
            let by_market = candidates
                .iter()
                .filter_map(|(position, player)| player.adp.map(|adp| (*position, adp)))
                .min_by(|a, b| a.1.total_cmp(&b.1))
                .map(|(position, _)| position);
            let by_points = || {
                candidates
                    .iter()
                    .max_by(|a, b| a.1.points.total_cmp(&b.1.points))
                    .map(|(position, _)| *position)
            };
            if let Some(position) = by_market.or_else(by_points) {
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

/// Assign tiers within a position. A new tier starts wherever the drop from
/// the previous player exceeds `gap_threshold`, *or* where the player sits
/// more than 1.5× that far below the top of the current tier — a smooth ramp
/// with no single big gap (every defense, the WR3–WR6 band) is still banded
/// rather than lumped into one 50-point "tier".
pub fn assign_tiers(sorted_points: &[f64], gap_threshold: f64) -> Vec<u32> {
    let spread_cap = gap_threshold * 1.5;
    let mut tiers = Vec::with_capacity(sorted_points.len());
    let mut tier = 1u32;
    let mut tier_top = sorted_points.first().copied().unwrap_or(0.0);
    for (i, &pts) in sorted_points.iter().enumerate() {
        if i > 0 {
            let gap = sorted_points[i - 1] - pts;
            if gap > gap_threshold || tier_top - pts > spread_cap {
                tier += 1;
                tier_top = pts;
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
            adp: None,
        }
    }

    fn mk(pos: &str, pts: f64, adp: f64) -> ScoredPlayer {
        ScoredPlayer {
            position: pos.into(),
            points: pts,
            adp: Some(adp),
        }
    }

    /// The 2026 draft in miniature: receivers project higher, but the room
    /// drafts running backs first. Flex demand must follow the room.
    #[test]
    fn flex_demand_follows_the_market_not_the_projection() {
        let roster: Vec<String> = ["RB", "WR", "FLEX", "BN"]
            .iter()
            .map(|s| s.to_string())
            .collect();
        let players = vec![
            mk("WR", 250.0, 30.0),
            mk("WR", 240.0, 40.0),
            mk("WR", 230.0, 50.0),
            mk("WR", 220.0, 60.0),
            mk("RB", 200.0, 10.0),
            mk("RB", 190.0, 15.0),
            mk("RB", 180.0, 20.0),
            mk("RB", 170.0, 25.0),
        ];
        let model = compute_replacement(&players, &RosterRules::new(&roster), 2);
        // Both flex slots go to RB: after the two dedicated starters, RB3 and
        // RB4 (ADP 20, 25) are drafted before WR3 (ADP 50).
        assert_eq!(model.demand.get("RB"), Some(&4));
        assert_eq!(model.demand.get("WR"), Some(&2));
        // So replacement RB is deep and replacement WR is shallow, which is
        // what makes a back worth more than his raw points say.
        assert!(model.baseline["RB"] < model.baseline["WR"]);
    }

    #[test]
    fn without_market_data_projection_still_decides() {
        let roster: Vec<String> = ["RB", "WR", "FLEX", "BN"]
            .iter()
            .map(|s| s.to_string())
            .collect();
        // RB2 carries an ADP; the receiver in line does not, and never wins
        // on projection alone against a candidate the market has priced.
        let players = vec![
            mk("RB", 200.0, 10.0),
            mk("RB", 150.0, 80.0),
            mk("WR", 210.0, 20.0),
            sp("WR", 205.0),
        ];
        let model = compute_replacement(&players, &RosterRules::new(&roster), 1);
        assert_eq!(model.demand.get("RB"), Some(&2), "{:?}", model.demand);
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

    // Gap-only clustering never breaks a smooth ramp: all 32 defenses (124 →
    // 74 points, no 8-point gap anywhere) landed in tier 1, and 149 WRs in
    // tier 4. A tier whose top and bottom are 50 points apart is not a tier.
    #[test]
    fn a_smooth_ramp_is_still_split_into_bands() {
        let pts: Vec<f64> = (0..32).map(|i| 124.0 - 1.6 * i as f64).collect();
        let tiers = assign_tiers(&pts, 8.0);
        assert!(
            tiers.last().copied().unwrap_or(1) > 1,
            "one tier for 32 DEF: {tiers:?}"
        );
        for tier in 1..=tiers[tiers.len() - 1] {
            let members: Vec<f64> = pts
                .iter()
                .zip(&tiers)
                .filter(|(_, &t)| t == tier)
                .map(|(&p, _)| p)
                .collect();
            let spread = members[0] - members[members.len() - 1];
            assert!(
                spread <= 12.0,
                "tier {tier} spans {spread} points: {members:?}"
            );
        }
    }

    #[test]
    fn tiers_never_decrease_and_start_at_one() {
        let pts: Vec<f64> = (0..150).map(|i| 300.0 - 1.9 * i as f64).collect();
        let tiers = assign_tiers(&pts, 12.0);
        assert_eq!(tiers[0], 1);
        assert!(tiers.windows(2).all(|w| w[1] == w[0] || w[1] == w[0] + 1));
        assert!(
            *tiers.last().unwrap() >= 8,
            "150 WRs over 285 points: {tiers:?}"
        );
    }

    #[test]
    fn tiers_break_on_gaps() {
        let pts = vec![300.0, 295.0, 250.0, 248.0, 246.0, 200.0];
        let tiers = assign_tiers(&pts, 20.0);
        assert_eq!(tiers, vec![1, 1, 2, 2, 2, 3]);
    }
}
