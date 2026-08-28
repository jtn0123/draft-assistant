//! Snake-draft state: pick order math, per-team rosters, on-clock tracking,
//! and ADP-based survival probabilities.

use crate::sleeper::Pick;
use serde::Serialize;

/// Which slot (1-based) is on the clock at a given overall pick (1-based)?
pub fn slot_for_pick(pick_no: u32, teams: u32) -> u32 {
    let round = (pick_no - 1) / teams; // 0-based round
    let idx = (pick_no - 1) % teams; // 0-based index within round
    if round % 2 == 0 {
        idx + 1
    } else {
        teams - idx
    }
}

/// All overall pick numbers (1-based) belonging to a slot.
pub fn picks_for_slot(slot: u32, teams: u32, rounds: u32) -> Vec<u32> {
    (1..=teams * rounds)
        .filter(|&p| slot_for_pick(p, teams) == slot)
        .collect()
}

#[derive(Debug, Clone, Serialize)]
pub struct RosterEntry {
    pub player_id: String,
    pub name: String,
    pub position: String,
    pub team: Option<String>,
    pub pick_no: u32,
    pub round: u32,
}

#[derive(Debug, Clone, Serialize)]
pub struct TeamRoster {
    pub slot: u32,
    pub display_name: Option<String>,
    pub players: Vec<RosterEntry>,
    /// Open starting slots by label, e.g. {"RB": 1, "FLEX": 2}
    pub open_starters: Vec<(String, u32)>,
}

/// Fill starters greedily in roster_positions order: dedicated slots first,
/// then flex takes leftover eligible players. Returns open (unfilled) slots.
pub fn open_starting_slots(
    roster_positions: &[String],
    players: &[RosterEntry],
) -> Vec<(String, u32)> {
    use std::collections::HashMap;
    let mut have: HashMap<&str, u32> = HashMap::new();
    for p in players {
        *have.entry(p.position.as_str()).or_insert(0) += 1;
    }
    let mut open: HashMap<String, u32> = HashMap::new();
    // Dedicated slots consume their own position first.
    for slot in roster_positions {
        if slot == "BN" {
            continue;
        }
        if crate::valuation::flex_eligible(slot).is_none() {
            let n = have.entry(slot.as_str()).or_insert(0);
            if *n > 0 {
                *n -= 1;
            } else {
                *open.entry(slot.clone()).or_insert(0) += 1;
            }
        }
    }
    // Flex slots consume whatever eligible players remain.
    for slot in roster_positions {
        if let Some(elig) = crate::valuation::flex_eligible(slot) {
            let mut filled = false;
            for pos in elig {
                let n = have.entry(pos).or_insert(0);
                if *n > 0 {
                    *n -= 1;
                    filled = true;
                    break;
                }
            }
            if !filled {
                *open.entry(slot.clone()).or_insert(0) += 1;
            }
        }
    }
    // Stable, readable order: starters in league order, deduped.
    let mut out: Vec<(String, u32)> = Vec::new();
    for slot in roster_positions {
        if let Some(n) = open.remove(slot) {
            out.push((slot.clone(), n));
        }
    }
    out
}

/// P(player still available at overall pick `at_pick`), given their ADP.
/// Selection pick modeled as Normal(adp, sigma) with sigma growing with ADP.
pub fn survival_probability(adp: f64, at_pick: u32) -> f64 {
    if adp <= 0.0 || adp >= 500.0 {
        // No real ADP signal — assume safe.
        return 0.99;
    }
    let sigma = (0.22 * adp).max(3.0);
    let z = (at_pick as f64 - adp) / sigma;
    (1.0 - crate::scoring::norm_cdf(z)).clamp(0.01, 0.99)
}

/// Group picks into per-slot rosters.
pub fn build_rosters(
    picks: &[Pick],
    teams: u32,
    roster_positions: &[String],
    slot_names: &std::collections::HashMap<u32, String>,
    name_of: impl Fn(&str) -> (String, String, Option<String>),
) -> Vec<TeamRoster> {
    let mut rosters: Vec<TeamRoster> = (1..=teams)
        .map(|slot| TeamRoster {
            slot,
            display_name: slot_names.get(&slot).cloned(),
            players: Vec::new(),
            open_starters: Vec::new(),
        })
        .collect();
    for pick in picks {
        let slot = pick.draft_slot;
        if slot == 0 || slot > teams {
            continue;
        }
        let (name, position, team) = name_of(&pick.player_id);
        rosters[(slot - 1) as usize].players.push(RosterEntry {
            player_id: pick.player_id.clone(),
            name,
            position,
            team,
            pick_no: pick.pick_no,
            round: pick.round,
        });
    }
    for roster in &mut rosters {
        roster.open_starters = open_starting_slots(roster_positions, &roster.players);
    }
    rosters
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn snake_order_14_teams() {
        assert_eq!(slot_for_pick(1, 14), 1);
        assert_eq!(slot_for_pick(2, 14), 2);
        assert_eq!(slot_for_pick(14, 14), 14);
        assert_eq!(slot_for_pick(15, 14), 14); // snake turn
        assert_eq!(slot_for_pick(27, 14), 2);
        assert_eq!(slot_for_pick(28, 14), 1);
        assert_eq!(slot_for_pick(29, 14), 1); // next turn
        assert_eq!(slot_for_pick(30, 14), 2);
    }

    #[test]
    fn slot2_pick_numbers_match_league_doc() {
        // From the spec: slot 2 in a 14-team snake.
        let picks = picks_for_slot(2, 14, 15);
        assert_eq!(
            picks,
            vec![2, 27, 30, 55, 58, 83, 86, 111, 114, 139, 142, 167, 170, 195, 198]
        );
    }

    #[test]
    fn survival_extremes() {
        // ADP 1 player is nearly gone by pick 27.
        assert!(survival_probability(1.5, 27) < 0.05);
        // ADP 100 player is nearly certain at pick 27.
        assert!(survival_probability(100.0, 27) > 0.95);
    }

    #[test]
    fn open_slots_fill_flex_after_dedicated() {
        let roster: Vec<String> = ["QB", "RB", "WR", "TE", "FLEX", "FLEX", "FLEX", "FLEX", "DEF", "BN"]
            .iter()
            .map(|s| s.to_string())
            .collect();
        let players = vec![
            RosterEntry { player_id: "a".into(), name: "A".into(), position: "RB".into(), team: None, pick_no: 2, round: 1 },
            RosterEntry { player_id: "b".into(), name: "B".into(), position: "RB".into(), team: None, pick_no: 27, round: 2 },
            RosterEntry { player_id: "c".into(), name: "C".into(), position: "WR".into(), team: None, pick_no: 30, round: 3 },
        ];
        let open = open_starting_slots(&roster, &players);
        // RB and WR dedicated slots filled; 1 RB spills into flex.
        let as_map: std::collections::HashMap<_, _> = open.into_iter().collect();
        assert_eq!(as_map.get("QB"), Some(&1));
        assert_eq!(as_map.get("TE"), Some(&1));
        assert_eq!(as_map.get("DEF"), Some(&1));
        assert_eq!(as_map.get("FLEX"), Some(&3));
        assert_eq!(as_map.get("RB"), None);
        assert_eq!(as_map.get("WR"), None);
    }
}
