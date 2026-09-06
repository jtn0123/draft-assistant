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

    /// Bench, taxi and every flavour of injured reserve. Yahoo spells its
    /// reserve slot "IR+" and Sleeper leagues have been seen with "IR2";
    /// matching "IR" alone counted those as starting slots, so a roster with
    /// two of them read as two open starters all draft.
    pub fn is_non_starting(slot: &str) -> bool {
        matches!(slot, "BN" | "TAXI") || slot.starts_with("IR")
    }

    /// A starting slot no position this board drafts can fill: the IDP
    /// slots (DL, LB, DB, IDP_FLEX) and anything else Sleeper adds. The board
    /// carries no players for them, so they are neither open starters nor
    /// need pressure; counting them as both told a twelve-round IDP draft
    /// that seven starters were open when four were.
    pub fn is_unfillable(slot: &str) -> bool {
        !Self::is_non_starting(slot)
            && Self::flex_eligible(slot).is_none()
            && !DRAFTABLE.contains(&slot)
    }

    /// A starting slot this board can build for: not bench or reserve, and
    /// not one of the IDP slots the board has no players for. Every place
    /// that counts open starters, starting demand or starters' byes asks this
    /// one question; three of them used to ask only `is_non_starting`, so an
    /// IDP league's DL and LB slots were excluded from the draft cards and
    /// still counted at replacement level and in the bye clash line.
    pub fn counts_as_open_starter(slot: &str) -> bool {
        !Self::is_non_starting(slot) && !Self::is_unfillable(slot)
    }

    /// The distinct unfillable slots on this roster, in roster order, for
    /// the warning that says the board is not drafting for them.
    pub fn unfillable_slots(&self) -> Vec<String> {
        let mut seen: Vec<String> = Vec::new();
        for slot in &self.slots {
            if Self::is_unfillable(slot) && !seen.contains(slot) {
                seen.push(slot.clone());
            }
        }
        seen
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
            if !Self::counts_as_open_starter(slot) || Self::flex_eligible(slot).is_some() {
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
    fn every_flavour_of_injured_reserve_is_a_non_starting_slot() {
        // Yahoo's "IR+" and a second Sleeper reserve slot both read as open
        // starters, and the draft advice chased bodies to fill them.
        for slot in ["IR", "IR+", "IR2", "BN", "TAXI"] {
            assert!(RosterRules::is_non_starting(slot), "{slot}");
            assert!(!RosterRules::can_fill(slot, "RB"), "{slot}");
        }
        assert!(!RosterRules::is_non_starting("RB"));
        let open = rules(&["RB", "IR+", "IR2"]).open_starting_slots(["RB"]);
        assert!(open.is_empty(), "{open:?}");
    }

    #[test]
    fn idp_slots_are_named_as_unfillable_and_never_counted_as_open_starters() {
        // An IDP league: the board has no DL, LB or DB, so those slots stay
        // "open" for the whole draft and inflate the need pressure on every
        // card. They are reported once and left out of the open count.
        let idp = rules(&["QB", "RB", "DL", "LB", "LB", "DB", "IDP_FLEX", "BN", "IR"]);
        assert_eq!(idp.unfillable_slots(), vec!["DL", "LB", "DB", "IDP_FLEX"]);
        let open = idp.open_starting_slots(["QB"]);
        assert_eq!(open, vec![("RB".to_string(), 1)]);
        assert!(
            rules(&["QB", "FLEX", "SUPER_FLEX", "K", "DEF", "BN", "IR+"])
                .unfillable_slots()
                .is_empty()
        );
        assert!(!RosterRules::is_unfillable("BN"));
        assert!(!RosterRules::is_unfillable("FLEX"));
    }

    #[test]
    fn an_idp_slot_never_counts_as_an_open_starter_anywhere_the_question_is_asked() {
        // The one helper every open-starter count goes through. A site that
        // asked only "is it bench?" counted DL and LB as starters to fill.
        for slot in ["DL", "LB", "DB", "IDP_FLEX", "BN", "IR", "IR+", "TAXI"] {
            assert!(!RosterRules::counts_as_open_starter(slot), "{slot}");
        }
        for slot in ["QB", "RB", "WR", "TE", "K", "DEF", "FLEX", "SUPER_FLEX"] {
            assert!(RosterRules::counts_as_open_starter(slot), "{slot}");
        }
    }

    #[test]
    fn constrained_flex_fills_before_superflex_regardless_of_slot_order() {
        let rules = rules(&["SUPER_FLEX", "REC_FLEX"]);
        let open = rules.open_starting_slots(["QB", "WR"]);
        assert!(open.is_empty(), "{open:?}");
    }
}
