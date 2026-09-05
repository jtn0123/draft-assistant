//! One authoritative interpretation of Sleeper roster slots.

use std::collections::HashMap;

const FLEX: &[&str] = &["RB", "WR", "TE"];
const WR_RB_FLEX: &[&str] = &["RB", "WR"];
const REC_FLEX: &[&str] = &["WR", "TE"];
const SUPER_FLEX: &[&str] = &["QB", "RB", "WR", "TE"];
const DRAFTABLE: &[&str] = &["QB", "RB", "WR", "TE", "K", "DEF"];

#[derive(Debug, Clone)]
pub struct RosterRules {
    slots: Vec<String>,
}

impl RosterRules {
    pub fn new(slots: &[String]) -> Self {
        Self {
            slots: slots.to_vec(),
        }
    }

    pub fn slots(&self) -> &[String] {
        &self.slots
    }

    pub fn flex_eligible(slot: &str) -> Option<&'static [&'static str]> {
        match slot {
            "FLEX" => Some(FLEX),
            "WRRB_FLEX" => Some(WR_RB_FLEX),
            "REC_FLEX" => Some(REC_FLEX),
            "SUPER_FLEX" => Some(SUPER_FLEX),
            _ => None,
        }
    }

    pub fn is_non_starting(slot: &str) -> bool {
        matches!(slot, "BN" | "IR" | "TAXI")
    }

    pub fn can_fill(slot: &str, position: &str) -> bool {
        Self::flex_eligible(slot)
            .map(|eligible| eligible.contains(&position))
            .unwrap_or_else(|| !Self::is_non_starting(slot) && slot == position)
    }

    pub fn draftable_positions(&self) -> Vec<String> {
        DRAFTABLE
            .iter()
            .filter(|position| self.slots.iter().any(|slot| Self::can_fill(slot, position)))
            .map(|position| (*position).to_string())
            .collect()
    }

    /// Which eligible position a flex slot goes to, out of the ones `wants`
    /// says are still in the market for it.
    ///
    /// The eligibility lists above are written best-first, so the answer is
    /// simply the first one that wants it: a SUPER_FLEX goes to the
    /// quarterback whenever a quarterback is in play, because a second
    /// quarterback in a superflex league is worth more in that slot than any
    /// receiver is. The rule this replaces was "whichever position has the
    /// most spare bodies", which is a count and not a value, and would hand a
    /// superflex slot to a fourth receiver over a second quarterback purely
    /// because there were more receivers lying around.
    pub fn flex_claimant(slot: &str, wants: impl Fn(&str) -> bool) -> Option<&'static str> {
        Self::flex_eligible(slot)?
            .iter()
            .copied()
            .find(|position| wants(position))
    }

    pub fn open_starting_slots<'a>(
        &self,
        player_positions: impl IntoIterator<Item = &'a str>,
    ) -> Vec<(String, u32)> {
        let mut remaining: HashMap<&str, u32> = HashMap::new();
        for position in player_positions {
            *remaining.entry(position).or_insert(0) += 1;
        }
        let mut open: HashMap<String, u32> = HashMap::new();

        for slot in &self.slots {
            if Self::is_non_starting(slot) || Self::flex_eligible(slot).is_some() {
                continue;
            }
            let count = remaining.entry(slot.as_str()).or_insert(0);
            if *count > 0 {
                *count -= 1;
            } else {
                *open.entry(slot.clone()).or_insert(0) += 1;
            }
        }

        let mut flex_slots = self
            .slots
            .iter()
            .filter(|slot| Self::flex_eligible(slot).is_some())
            .collect::<Vec<_>>();
        flex_slots.sort_by_key(|slot| Self::flex_eligible(slot).map_or(0, <[&str]>::len));
        for slot in flex_slots {
            let claimant = Self::flex_claimant(slot, |position| {
                remaining.get(position).copied().unwrap_or(0) > 0
            });
            if let Some(position) = claimant {
                *remaining.entry(position).or_insert(0) -= 1;
            } else {
                *open.entry(slot.clone()).or_insert(0) += 1;
            }
        }

        let mut result = Vec::new();
        for slot in &self.slots {
            if let Some(count) = open.remove(slot) {
                result.push((slot.clone(), count));
            }
        }
        result
    }

    pub fn first_open_slot_for(
        &self,
        open_slots: &HashMap<String, u32>,
        position: &str,
    ) -> Option<&str> {
        self.slots
            .iter()
            .find(|slot| {
                open_slots.get(*slot).copied().unwrap_or(0) > 0 && Self::can_fill(slot, position)
            })
            .map(String::as_str)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn rules(slots: &[&str]) -> RosterRules {
        RosterRules::new(
            &slots
                .iter()
                .map(|slot| (*slot).to_string())
                .collect::<Vec<_>>(),
        )
    }

    #[test]
    fn superflex_and_mixed_flex_have_distinct_eligibility() {
        assert!(RosterRules::can_fill("SUPER_FLEX", "QB"));
        assert!(!RosterRules::can_fill("REC_FLEX", "QB"));
        assert!(!RosterRules::can_fill("WRRB_FLEX", "TE"));
        assert!(RosterRules::can_fill("REC_FLEX", "TE"));
    }

    #[test]
    fn draftable_positions_include_kicker_only_when_rostered() {
        assert_eq!(
            rules(&["QB", "SUPER_FLEX", "K", "BN"]).draftable_positions(),
            vec!["QB", "RB", "WR", "TE", "K"]
        );
        assert!(!rules(&["QB", "FLEX", "BN"])
            .draftable_positions()
            .contains(&"K".to_string()));
    }

    #[test]
    fn a_superflex_slot_goes_to_the_quarterback_not_to_the_deepest_pile() {
        // Spare bodies at three positions, receivers the most numerous of
        // them. Counting bodies handed the slot to a receiver; what the slot
        // is worth says quarterback.
        let spare = |position: &str| matches!(position, "QB" | "WR" | "TE");
        assert_eq!(RosterRules::flex_claimant("SUPER_FLEX", spare), Some("QB"));
        // And a flex a quarterback cannot fill still goes by value order.
        assert_eq!(RosterRules::flex_claimant("FLEX", spare), Some("WR"));
        assert_eq!(
            RosterRules::flex_claimant("REC_FLEX", |p| p == "TE"),
            Some("TE")
        );
        assert_eq!(RosterRules::flex_claimant("QB", spare), None);
    }

    #[test]
    fn constrained_flex_fills_before_superflex_regardless_of_slot_order() {
        let rules = rules(&["SUPER_FLEX", "REC_FLEX"]);
        let open = rules.open_starting_slots(["QB", "WR"]);
        assert!(open.is_empty(), "{open:?}");
    }
}
