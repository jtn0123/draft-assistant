//! Who actually picks at pick N.
//!
//! The snake says which *slot* a pick belongs to. In a league that trades
//! draft picks that is only where the pick started: Sleeper's
//! `/draft/{id}/traded_picks` says which roster owns it now, and
//! `slot_to_roster_id` on the draft maps rosters back to slots. A league with
//! forty such trades had a quarter of its board made by someone other than
//! the slot's owner — every opponent roster drawn from slots alone was wrong,
//! and so was the manager named on the clock.
//!
//! Without the trade list (an older cache, a mock draft, a failed fetch) this
//! degrades to the plain snake, which is what the app did before.

use crate::draft::{self, DraftOrder};
use crate::sleeper::{Draft, SleeperClient, BASE};
use crate::sleeper_error::SleeperError;
use serde::{Deserialize, Serialize};
use std::collections::HashMap;

/// One entry from `/draft/{id}/traded_picks`: the pick that roster
/// `roster_id` was due in `round` now belongs to roster `owner_id`.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct TradedPick {
    pub season: String,
    pub round: u32,
    /// The roster the pick originally belonged to.
    pub roster_id: u32,
    /// The roster that owns it now.
    pub owner_id: u32,
    #[serde(default)]
    pub previous_owner_id: Option<u32>,
}

impl SleeperClient {
    /// The draft's traded picks. An empty list is the normal answer for a
    /// league that trades none, and for every mock draft.
    pub async fn traded_picks(&self, draft_id: &str) -> Result<Vec<TradedPick>, SleeperError> {
        let v: Option<Vec<TradedPick>> = self
            .get_json(&format!("{BASE}/draft/{draft_id}/traded_picks"))
            .await?;
        Ok(v.unwrap_or_default())
    }
}

/// The snake order corrected for trades: pick number -> the slot whose
/// manager makes it.
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

    /// The plain snake, with no trades and no slot map — what a mock draft
    /// and every test that does not care about ownership wants.
    pub fn plain(teams: u32, rounds: u32, order: DraftOrder) -> Self {
        Self {
            teams,
            rounds,
            order,
            slot_to_roster: HashMap::new(),
            roster_to_slot: HashMap::new(),
            traded: HashMap::new(),
        }
    }

    /// The slot whose manager makes this pick. Falls back to the slot the
    /// snake gives whenever the trade list cannot be applied.
    pub fn owner_slot(&self, pick_no: u32) -> Option<u32> {
        let origin = draft::slot_for_pick(pick_no, self.teams, self.order)?;
        if self.traded.is_empty() {
            return Some(origin);
        }
        let round = (pick_no - 1) / self.teams + 1;
        let Some(roster) = self.slot_to_roster.get(&origin) else {
            return Some(origin);
        };
        Some(
            self.traded
                .get(&(round, *roster))
                .and_then(|owner| self.roster_to_slot.get(owner))
                .copied()
                .unwrap_or(origin),
        )
    }

    /// Every pick this slot's manager will make, trades included.
    pub fn picks_owned_by(&self, slot: u32) -> Vec<u32> {
        (1..=self.teams.saturating_mul(self.rounds))
            .filter(|&p| self.owner_slot(p) == Some(slot))
            .collect()
    }

    /// Only the picks the plain snake gets wrong: pick number -> owner slot.
    ///
    /// This is what the frontend draws its queue from. It covers third-round
    /// reversal as well as trades, because the frontend's own snake
    /// arithmetic knows about neither — anything that disagrees with a plain
    /// snake ends up here.
    pub fn overrides(&self) -> HashMap<u32, u32> {
        (1..=self.teams.saturating_mul(self.rounds))
            .filter_map(|p| {
                let owner = self.owner_slot(p)?;
                (Some(owner) != draft::slot_for_pick(p, self.teams, DraftOrder::SNAKE))
                    .then_some((p, owner))
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

    /// 14 teams; slot 13 is roster 11 and slot 5 is roster 2.
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
        assert_eq!(draft::slot_for_pick(153, 14, DraftOrder::SNAKE), Some(13));
        assert_eq!(own.owner_slot(153), Some(5));
        // Pick 164 is round 12, slot 5.
        assert_eq!(draft::slot_for_pick(164, 14, DraftOrder::SNAKE), Some(5));
        assert_eq!(own.owner_slot(164), Some(13));
        // Untraded picks are untouched.
        assert_eq!(own.owner_slot(145), Some(5));
        assert_eq!(own.overrides(), HashMap::from([(153, 5), (164, 13)]));
    }

    #[test]
    fn a_slot_owns_its_own_picks_plus_the_ones_it_gained_minus_the_ones_it_sent() {
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
        assert_eq!(own.owner_slot(153), Some(13));
        assert!(own.overrides().is_empty());
        // Trades but no slot map (a mock draft): nothing to translate through.
        let mut bare = league();
        bare.slot_to_roster_id = None;
        let own = PickOwnership::from_draft(&bare, &[traded(11, 11, 2)], 14, 15, DraftOrder::SNAKE);
        assert_eq!(own.owner_slot(153), Some(13));
    }

    #[test]
    fn another_seasons_trade_is_ignored() {
        let mut next_year = traded(11, 11, 2);
        next_year.season = "2027".into();
        let own = PickOwnership::from_draft(&league(), &[next_year], 14, 15, DraftOrder::SNAKE);
        assert_eq!(own.owner_slot(153), Some(13));
    }

    #[test]
    fn a_reversal_round_shows_up_in_the_overrides_the_frontend_reads() {
        let order = DraftOrder {
            linear: false,
            reversal_round: 3,
        };
        let own = PickOwnership::plain(14, 15, order);
        let overrides = own.overrides();
        // Rounds 1–2 match a plain snake and are absent; round 3 is flipped.
        assert!(!overrides.contains_key(&1) && !overrides.contains_key(&15));
        assert_eq!(overrides.get(&29), Some(&14));
        assert_eq!(own.owner_slot(29), Some(14));
    }

    #[test]
    fn a_draft_with_no_teams_owns_nothing_rather_than_panicking() {
        let own = PickOwnership::plain(0, 15, DraftOrder::SNAKE);
        assert_eq!(own.owner_slot(1), None);
        assert!(own.picks_owned_by(1).is_empty());
        assert!(own.overrides().is_empty());
    }
}
