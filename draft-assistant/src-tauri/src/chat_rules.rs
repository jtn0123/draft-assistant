//! The house rules of a draft, read back out of the view.
//!
//! `DraftView` carries the *effects* of a league's rules — `keeper_picks`,
//! `pick_slot_overrides` — rather than the rules themselves, because that is
//! all the board needs to draw itself correctly. Claude needs the rule: "you
//! have two keepers and no second-rounder" changes an answer in a way that a
//! map of pick numbers does not. This module reads one back out of the other.

use crate::view::DraftView;

/// What the draft order and the pick book say the league is playing.
#[derive(Debug, Clone, PartialEq, Eq, Default)]
pub struct LeagueRules {
    /// The round third-round reversal begins from, when the order shows one.
    pub reversal_round: Option<u32>,
    /// Every pick already spent on a keeper, league-wide.
    pub keepers_total: usize,
    /// Those of them that are yours.
    pub my_keeper_picks: Vec<u32>,
    /// Picks you own that the draft order alone would not have given you.
    pub picks_gained: Vec<u32>,
    /// Picks the draft order gave you that somebody else owns now.
    pub picks_lost: Vec<u32>,
}

impl LeagueRules {
    /// True when nothing here is worth a line in the prompt.
    pub fn is_plain(&self) -> bool {
        self.reversal_round.is_none()
            && self.keepers_total == 0
            && self.picks_gained.is_empty()
            && self.picks_lost.is_empty()
    }
}

/// The slot a plain snake gives this pick — the same arithmetic the frontend
/// does, and the baseline `pick_slot_overrides` is a diff against.
fn snake_slot(pick_no: u32, teams: u32) -> u32 {
    let round = (pick_no - 1) / teams;
    let index = (pick_no - 1) % teams;
    if round.is_multiple_of(2) {
        index + 1
    } else {
        teams - index
    }
}

/// Third-round reversal flips every round from the reversal round on, so a
/// reversed round's slots are the plain snake's read backwards.
fn mirrored(slot: u32, teams: u32) -> u32 {
    teams + 1 - slot
}

pub fn league_rules(view: &DraftView) -> LeagueRules {
    let teams = view.draft.teams;
    let rounds = view.draft.rounds;
    if teams == 0 || rounds == 0 {
        return LeagueRules::default();
    }
    let owner = |pick_no: u32| -> u32 {
        view.draft
            .pick_slot_overrides
            .get(&pick_no)
            .copied()
            .unwrap_or_else(|| snake_slot(pick_no, teams))
    };

    // A reversal shows up as a whole round reading backwards. Trades move a
    // pick or two, so a round counts as flipped on a majority rather than on
    // every pick, and the reversal is the earliest round from which every
    // later round is flipped too.
    let flipped = |round: u32| -> bool {
        let first = (round - 1) * teams + 1;
        let matches = (first..first + teams)
            .filter(|&p| owner(p) == mirrored(snake_slot(p, teams), teams))
            .count();
        matches * 2 > teams as usize
    };
    let mut reversal_round = None;
    for round in (2..=rounds).rev() {
        if !flipped(round) {
            break;
        }
        reversal_round = Some(round);
    }

    // With the reversal known, the remaining disagreements are trades.
    let expected = |pick_no: u32| -> u32 {
        let round = (pick_no - 1) / teams + 1;
        let plain = snake_slot(pick_no, teams);
        match reversal_round {
            Some(from) if round >= from => mirrored(plain, teams),
            _ => plain,
        }
    };

    let keepers: std::collections::HashSet<u32> = view.draft.keeper_picks.iter().copied().collect();
    let mut rules = LeagueRules {
        reversal_round,
        keepers_total: keepers.len(),
        ..LeagueRules::default()
    };
    let Some(my_slot) = view.draft.my_slot else {
        return rules;
    };
    rules.my_keeper_picks = view
        .draft
        .keeper_picks
        .iter()
        .copied()
        .filter(|&p| owner(p) == my_slot)
        .collect();
    rules.my_keeper_picks.sort_unstable();
    // A keeper's pick is nobody's to make, so it is neither gained nor lost.
    for pick in 1..=teams * rounds {
        if keepers.contains(&pick) {
            continue;
        }
        match (owner(pick) == my_slot, expected(pick) == my_slot) {
            (true, false) => rules.picks_gained.push(pick),
            (false, true) => rules.picks_lost.push(pick),
            _ => {}
        }
    }
    rules
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_plain_snake_has_no_rules_to_report() {
        let view = crate::chat_fixtures::draft_fixture();
        let rules = league_rules(&view);
        assert!(rules.is_plain(), "{rules:?}");
        assert_eq!(rules.reversal_round, None);
    }

    #[test]
    fn a_reversal_round_is_read_back_out_of_the_overrides() {
        let mut view = crate::chat_fixtures::draft_fixture();
        let teams = view.draft.teams;
        // Rounds 3 upward read backwards, which is what the view stores for a
        // league with `reversal_round: 3`.
        for round in 3..=view.draft.rounds {
            for index in 0..teams {
                let pick = (round - 1) * teams + index + 1;
                let plain = snake_slot(pick, teams);
                view.draft
                    .pick_slot_overrides
                    .insert(pick, teams + 1 - plain);
            }
        }
        let rules = league_rules(&view);
        assert_eq!(rules.reversal_round, Some(3));
        // The reversal alone is not a trade.
        assert!(rules.picks_gained.is_empty(), "{rules:?}");
        assert!(rules.picks_lost.is_empty(), "{rules:?}");
    }

    #[test]
    fn a_trade_on_top_of_a_reversal_is_still_a_trade() {
        let mut view = crate::chat_fixtures::draft_fixture();
        let teams = view.draft.teams;
        for round in 3..=view.draft.rounds {
            for index in 0..teams {
                let pick = (round - 1) * teams + index + 1;
                let plain = snake_slot(pick, teams);
                view.draft
                    .pick_slot_overrides
                    .insert(pick, teams + 1 - plain);
            }
        }
        // My slot is 3. Give away my second-rounder and take somebody's third.
        let my_second = teams + (teams - 3 + 1);
        assert_eq!(snake_slot(my_second, teams), 3);
        view.draft.pick_slot_overrides.insert(my_second, 1);
        let their_third = 2 * teams + 1;
        view.draft.pick_slot_overrides.insert(their_third, 3);

        let rules = league_rules(&view);
        assert_eq!(rules.reversal_round, Some(3));
        assert_eq!(rules.picks_lost, vec![my_second]);
        assert_eq!(rules.picks_gained, vec![their_third]);
    }

    #[test]
    fn keepers_are_counted_league_wide_and_named_when_they_are_mine() {
        let mut view = crate::chat_fixtures::draft_fixture();
        // Pick 3 is my slot's first-rounder; pick 1 is somebody else's.
        view.draft.keeper_picks = vec![1, 3];
        let rules = league_rules(&view);
        assert_eq!(rules.keepers_total, 2);
        assert_eq!(rules.my_keeper_picks, vec![3]);
        // A keeper is not a lost pick.
        assert!(rules.picks_lost.is_empty(), "{rules:?}");
    }

    #[test]
    fn an_empty_draft_is_not_a_panic() {
        let mut view = crate::chat_fixtures::draft_fixture();
        view.draft.teams = 0;
        assert!(league_rules(&view).is_plain());
    }
}
