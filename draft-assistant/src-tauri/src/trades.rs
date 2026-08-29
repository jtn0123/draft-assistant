//! Who actually picks at pick N.
//!
//! The snake says which *slot* a pick belongs to. In a league that trades
//! draft picks that is only where the pick started: Sleeper's
//! `/draft/{id}/traded_picks` says which roster owns it now, and
//! `slot_to_roster_id` on the draft maps rosters back to slots. Tonight's
//! league had 40 such trades, and 36 of the first 150 picks were made by
//! someone other than the slot's owner — every opponent roster the app drew
//! from slots alone was wrong, and the manager it named on the clock was
//! wrong for a fifth of the board.
//!
//! Without the trade list (an older cache, a mock draft, a failed fetch) this
//! degrades to the plain snake, which is exactly what the app did before.

use crate::draft::{self, DraftOrder};
use crate::sleeper::{Draft, TradedPick};
use std::collections::HashMap;

#[derive(Debug, Clone)]
pub struct PickOwnership {
    teams: u32,
    rounds: u32,
    order: DraftOrder,
    slot_to_roster: HashMap<u32, u32>,
    roster_to_slot: HashMap<u32, u32>,
    /// (round, roster the pick started with) -> roster that owns it now.
    traded: HashMap<(u32, u32), u32>,
}

impl PickOwnership {
    pub fn from_draft(
        draft: &Draft,
        traded: &[TradedPick],
        teams: u32,
        rounds: u32,
        order: DraftOrder,
    ) -> Self {
        let slot_to_roster: HashMap<u32, u32> = draft
            .slot_to_roster_id
            .as_ref()
            .map(|m| {
                m.iter()
                    .filter_map(|(slot, roster)| slot.parse::<u32>().ok().map(|s| (s, *roster)))
                    .collect()
            })
            .unwrap_or_default();
        let roster_to_slot = slot_to_roster.iter().map(|(s, r)| (*r, *s)).collect();
        // The draft endpoint already scopes trades to this draft; the season
        // check is belt and braces against a league-level list being passed.
        let traded = traded
            .iter()
            .filter(|t| draft.season.as_deref().is_none_or(|s| s == t.season))
            .map(|t| ((t.round, t.roster_id), t.owner_id))
            .collect();
        Self {
            teams,
            rounds,
            order,
            slot_to_roster,
            roster_to_slot,
            traded,
        }
    }

    /// The slot whose manager makes this pick.
    pub fn owner_slot(&self, pick_no: u32) -> u32 {
        let origin = draft::slot_for_pick(pick_no, self.teams, self.order);
        if self.traded.is_empty() || self.teams == 0 {
            return origin;
        }
        let round = (pick_no - 1) / self.teams + 1;
        let Some(roster) = self.slot_to_roster.get(&origin) else {
            return origin;
        };
        self.traded
            .get(&(round, *roster))
            .and_then(|owner| self.roster_to_slot.get(owner))
            .copied()
            .unwrap_or(origin)
    }

    /// Every pick this slot's manager will make, trades included.
    pub fn picks_owned_by(&self, slot: u32) -> Vec<u32> {
        (1..=self.teams.saturating_mul(self.rounds))
            .filter(|&p| self.owner_slot(p) == slot)
            .collect()
    }

    /// Only the picks that do not follow the snake: pick number -> owner slot.
    /// What the frontend needs to draw the strip without its own copy of the
    /// trade list.
    pub fn overrides(&self) -> HashMap<u32, u32> {
        if self.traded.is_empty() {
            return HashMap::new();
        }
        (1..=self.teams.saturating_mul(self.rounds))
            .filter_map(|p| {
                let owner = self.owner_slot(p);
                (owner != draft::slot_for_pick(p, self.teams, self.order)).then_some((p, owner))
            })
            .collect()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn traded(round: u32, roster_id: u32, owner_id: u32) -> TradedPick {
        TradedPick {
            season: "2026".into(),
            round,
            roster_id,
            owner_id,
            previous_owner_id: Some(roster_id),
        }
    }

    /// Tonight's league: 14 teams, slot 13 is roster 11, slot 5 is roster 2.
    fn league() -> Draft {
        serde_json::from_value(serde_json::json!({
            "draft_id": "d", "status": "drafting", "type": "snake", "season": "2026",
            "settings": {"teams": 14, "rounds": 15},
            "slot_to_roster_id": {"1": 14, "2": 13, "3": 9, "4": 10, "5": 2, "6": 8, "7": 6,
                                  "8": 4, "9": 5, "10": 3, "11": 7, "12": 1, "13": 11, "14": 12}
        }))
        .unwrap()
    }

    #[test]
    fn a_traded_pick_belongs_to_the_slot_that_owns_it_now() {
        // Round 11 from roster 11 (slot 13) went to roster 2 (slot 5), and
        // round 12 from roster 2 went the other way.
        let trades = [traded(11, 11, 2), traded(12, 2, 11)];
        let own = PickOwnership::from_draft(&league(), &trades, 14, 15, DraftOrder::SNAKE);
        // Pick 153 is round 11, slot 13 in a snake.
        assert_eq!(draft::slot_for_pick(153, 14, DraftOrder::SNAKE), 13);
        assert_eq!(own.owner_slot(153), 5);
        // Pick 164 is round 12, slot 5.
        assert_eq!(draft::slot_for_pick(164, 14, DraftOrder::SNAKE), 5);
        assert_eq!(own.owner_slot(164), 13);
        // Untraded picks are untouched.
        assert_eq!(own.owner_slot(145), 5);
        assert_eq!(own.overrides(), HashMap::from([(153, 5), (164, 13)]));
    }

    #[test]
    fn a_slot_owns_its_own_picks_plus_the_ones_it_acquired_minus_the_ones_it_sent() {
        let trades = [traded(11, 11, 2), traded(12, 2, 11)];
        let own = PickOwnership::from_draft(&league(), &trades, 14, 15, DraftOrder::SNAKE);
        let five = own.picks_owned_by(5);
        assert!(
            five.contains(&153),
            "gained round 11 from slot 13: {five:?}"
        );
        assert!(!five.contains(&164), "gave away round 12: {five:?}");
        assert_eq!(five.len(), 15);
        let thirteen = own.picks_owned_by(13);
        assert!(thirteen.contains(&164) && !thirteen.contains(&153));
    }

    #[test]
    fn without_trades_or_the_slot_map_it_is_the_plain_snake() {
        let own = PickOwnership::from_draft(&league(), &[], 14, 15, DraftOrder::SNAKE);
        assert_eq!(own.owner_slot(153), 13);
        assert!(own.overrides().is_empty());
        // Trades but no slot map (a mock draft): nothing to translate through.
        let mut bare = league();
        bare.slot_to_roster_id = None;
        let own = PickOwnership::from_draft(&bare, &[traded(11, 11, 2)], 14, 15, DraftOrder::SNAKE);
        assert_eq!(own.owner_slot(153), 13);
    }

    #[test]
    fn another_seasons_trade_is_ignored() {
        let mut next_year = traded(11, 11, 2);
        next_year.season = "2027".into();
        let own = PickOwnership::from_draft(&league(), &[next_year], 14, 15, DraftOrder::SNAKE);
        assert_eq!(own.owner_slot(153), 13);
    }
}
