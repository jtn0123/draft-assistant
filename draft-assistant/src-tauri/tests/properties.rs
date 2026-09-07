//! Property-based tests over the draft math.
//!
//! The rest of the suite pins known cases against the real league document;
//! these pin the *invariants* that must hold for every league shape Sleeper
//! can report, including shapes nobody has ever drafted. They exist because
//! the shapes that break this app are the ones no fixture thought to include:
//! a `teams: 0` payload panicked `build_view` through `slot_for_pick`, and a
//! `current_pick - 1` underflow crashed a release build, where
//! `overflow-checks` is on.
//!
//! Ported from an abandoned branch. Rewritten against today's API, which has
//! since made several of these invariants unbreakable by construction:
//! `slot_for_pick` returns `Option` rather than dividing by zero, and pick
//! ownership went through `PickOwnership`, which is what these now exercise.

use draft_assistant_lib::draft::{
    self, slot_for_pick, survival_probability, survival_probability_in, DraftOrder,
};
use draft_assistant_lib::roster::RosterRules;
use draft_assistant_lib::scoring::{base_points, norm_cdf};
use draft_assistant_lib::sleeper::Draft;
use draft_assistant_lib::traded_picks::PickOwnership;
use proptest::prelude::*;
use std::collections::{HashMap, HashSet};

/// Real leagues run 2 to 32 teams over 1 to 30 rounds; go wider on purpose.
fn teams() -> impl Strategy<Value = u32> {
    1u32..=40
}

fn rounds() -> impl Strategy<Value = u32> {
    1u32..=30
}

/// Snake, linear, and snake with a reversal in any early round.
fn order() -> impl Strategy<Value = DraftOrder> {
    (any::<bool>(), 0u32..8).prop_map(|(linear, reversal_round)| DraftOrder {
        linear,
        reversal_round,
    })
}

/// A draft with no traded picks and no slot map, so `PickOwnership` falls
/// through to the plain order. Built by deserializing, because that is how
/// every real one arrives and the only public way to make one.
fn ownership(teams: u32, rounds: u32, order: DraftOrder) -> PickOwnership {
    let draft: Draft = serde_json::from_str(&format!(
        r#"{{"draft_id":"d1","status":"drafting","type":"snake",
             "settings":{{"teams":{teams},"rounds":{rounds}}}}}"#
    ))
    .expect("the minimal draft payload parses");
    PickOwnership::from_draft(&draft, &[], teams, rounds, order)
}

proptest! {
    /// The clock must always land on a real team.
    #[test]
    fn the_clock_always_lands_on_a_real_slot(
        teams in teams(),
        rounds in rounds(),
        offset in 0u32..1200,
        order in order(),
    ) {
        let pick = (offset % (teams * rounds)) + 1;
        let slot = slot_for_pick(pick, teams, order).expect("a real pick has an owner");
        prop_assert!((1..=teams).contains(&slot), "slot {slot} outside 1..={teams}");
    }

    /// Snake: a slot's pick in an odd round and the next even round are
    /// mirrored, so the two slot numbers sum to teams + 1.
    #[test]
    fn snake_rounds_mirror_each_other(teams in 2u32..=40, idx in 0u32..40) {
        let idx = idx % teams;
        let first = slot_for_pick(idx + 1, teams, DraftOrder::SNAKE).unwrap();
        let second = slot_for_pick(teams + idx + 1, teams, DraftOrder::SNAKE).unwrap();
        prop_assert_eq!(first + second, teams + 1);
    }

    /// A pick before the first one, or a draft with no teams, is a question
    /// with no answer rather than a division by zero.
    #[test]
    fn a_draft_with_no_teams_answers_nothing_and_panics_at_nothing(
        pick in 0u32..100,
        teams in 0u32..3,
        order in order(),
    ) {
        match slot_for_pick(pick, teams, order) {
            Some(slot) => {
                prop_assert!(teams > 0 && pick > 0);
                prop_assert!((1..=teams).contains(&slot));
            }
            None => prop_assert!(teams == 0 || pick == 0),
        }
    }

    /// Every slot drafts once per round, and the per-slot lists partition the
    /// whole board with no gaps and no pick owned twice. Read through
    /// `PickOwnership`, which is what the view and the pick queue use.
    #[test]
    fn ownership_partitions_the_whole_board(
        teams in 1u32..=16,
        rounds in 1u32..=12,
        order in order(),
    ) {
        let owned = ownership(teams, rounds, order);
        let mut all = Vec::new();
        for slot in 1..=teams {
            let picks = owned.picks_owned_by(slot);
            prop_assert_eq!(
                picks.len(), rounds as usize,
                "slot {} got {} picks, expected {}", slot, picks.len(), rounds
            );
            for &pick in &picks {
                prop_assert_eq!(owned.owner_slot(pick), Some(slot));
            }
            all.extend(picks);
        }
        let unique: HashSet<u32> = all.iter().copied().collect();
        prop_assert_eq!(unique.len(), all.len(), "a pick belongs to two slots");
        prop_assert_eq!(all.len(), (teams * rounds) as usize);
    }

    /// Every override names a slot that exists, and none of them agrees with
    /// the plain snake the frontend draws: an override that matched would be
    /// noise on the wire, and one that named slot 0 would be a blank manager.
    #[test]
    fn every_override_disagrees_with_the_snake_and_names_a_real_slot(
        teams in 1u32..=16,
        rounds in 1u32..=12,
        order in order(),
    ) {
        for (pick, slot) in ownership(teams, rounds, order).overrides() {
            prop_assert!((1..=teams).contains(&slot), "override slot {slot} outside 1..={teams}");
            prop_assert_ne!(Some(slot), slot_for_pick(pick, teams, DraftOrder::SNAKE));
        }
    }

    /// A probability must be a probability, for any ADP and any pick.
    #[test]
    fn survival_is_always_a_probability(
        adp in -50.0f64..2000.0,
        at_pick in 0u32..1000,
        teams in 0u32..40,
    ) {
        for p in [survival_probability(adp, at_pick), survival_probability_in(adp, at_pick, teams)] {
            prop_assert!(p.is_finite(), "survival was {p}");
            prop_assert!((0.0..=1.0).contains(&p), "survival {p} outside [0,1]");
        }
    }

    /// Later picks can only make a player less likely to still be there.
    #[test]
    fn survival_never_rises_at_a_later_pick(
        adp in 1.0f64..300.0,
        at_pick in 1u32..300,
        ahead in 0u32..100,
    ) {
        let earlier = survival_probability(adp, at_pick + ahead);
        let later = survival_probability(adp, at_pick + ahead + 1);
        prop_assert!(later <= earlier + f64::EPSILON, "survival rose from {earlier} to {later}");
    }

    /// Keepers only ever move a pick earlier in the market: nobody selects at
    /// a keeper's number, so the count of real selections cannot exceed the
    /// overall pick, and cannot be zero for a pick that exists.
    #[test]
    fn a_keeper_never_pushes_a_pick_later_in_the_market(
        at_pick in 1u32..200,
        keepers in prop::collection::hash_set(1u32..200, 0..40),
    ) {
        let market = draft::market_pick(at_pick, &keepers);
        prop_assert!(market <= at_pick, "market pick {market} is past overall {at_pick}");
        prop_assert!(market >= 1, "every pick has a market position");
    }

    #[test]
    fn norm_cdf_is_a_distribution(z in -40.0f64..40.0) {
        let p = norm_cdf(z);
        prop_assert!(p.is_finite());
        prop_assert!((0.0..=1.0).contains(&p), "norm_cdf({z}) = {p}");
        // Symmetry: F(-z) = 1 - F(z).
        prop_assert!((norm_cdf(-z) - (1.0 - p)).abs() < 1e-9);
    }

    #[test]
    fn norm_cdf_is_monotonic(a in -20.0f64..20.0, delta in 0.0f64..20.0) {
        prop_assert!(norm_cdf(a + delta) >= norm_cdf(a) - 1e-12);
    }

    /// Scoring is a dot product over the league's own key space: a stat the
    /// league does not score contributes nothing, whatever its value.
    #[test]
    fn scoring_ignores_unscored_keys_and_stays_finite(
        pass_yd in 0.0f64..6000.0,
        rec in 0.0f64..200.0,
        junk in -1e6f64..1e6,
    ) {
        let mut stats = HashMap::new();
        stats.insert("pass_yd".to_string(), pass_yd);
        stats.insert("rec".to_string(), rec);
        let scoring: HashMap<String, f64> =
            [("pass_yd".to_string(), 0.04), ("rec".to_string(), 1.0)].into_iter().collect();

        let base = base_points(&stats, &scoring);
        prop_assert!(base.is_finite());

        stats.insert("not_a_scored_stat".to_string(), junk);
        prop_assert!((base_points(&stats, &scoring) - base).abs() < 1e-9);
    }

    /// Whatever roster shape a league declares, every position this app agrees
    /// to draft must be fillable by some slot in it.
    #[test]
    fn draftable_positions_are_all_actually_fillable(
        slots in prop::collection::vec(
            prop::sample::select(vec![
                "QB", "RB", "WR", "TE", "K", "DEF", "FLEX", "SUPER_FLEX",
                "WRRB_FLEX", "REC_FLEX", "BN", "IR", "TAXI",
            ]),
            0..16,
        )
    ) {
        let owned: Vec<String> = slots.iter().map(|s| (*s).to_string()).collect();
        for position in RosterRules::new(&owned).draftable_positions() {
            prop_assert!(
                owned.iter().any(|slot| RosterRules::can_fill(slot, &position)),
                "{position} is draftable but no slot can hold it"
            );
        }
    }

    /// Bench, IR and taxi slots never make a position draftable on their own.
    #[test]
    fn non_starting_slots_alone_draft_nothing(
        slots in prop::collection::vec(prop::sample::select(vec!["BN", "IR", "TAXI"]), 1..10)
    ) {
        let owned: Vec<String> = slots.iter().map(|s| (*s).to_string()).collect();
        prop_assert!(RosterRules::new(&owned).draftable_positions().is_empty());
    }
}

/// Randomized parsing robustness.
///
/// Every type below is deserialized straight from an undocumented third-party
/// API, so upstream payload drift is the realistic way this app breaks on
/// draft night. Hostile input must come back as `Err`, never as a panic.
mod parsing_robustness {
    use draft_assistant_lib::sleeper::{Draft, League, Pick, PlayerMeta, ProjectionRow};
    use proptest::prelude::*;

    /// Strings biased toward JSON: structural characters, the numeric edge
    /// cases serde has to reject, and the key names the real payloads use, so
    /// the generator reaches the parser rather than bouncing off the first
    /// byte.
    fn jsonish() -> impl Strategy<Value = String> {
        proptest::collection::vec(
            prop::sample::select(vec![
                "{",
                "}",
                "[",
                "]",
                ":",
                ",",
                "\"",
                "null",
                "true",
                "0",
                "-1",
                "1e400",
                "\"teams\"",
                "\"rounds\"",
                "\"player_id\"",
                "\"stats\"",
                "\"league_id\"",
                "\"roster_positions\"",
                "\"scoring_settings\"",
                "\"settings\"",
                "\"status\"",
                "\"type\"",
                "\"draft_id\"",
                "NaN",
                "Infinity",
            ]),
            0..40,
        )
        .prop_map(|parts| parts.concat())
    }

    proptest! {
        #![proptest_config(ProptestConfig::with_cases(2048))]

        #[test]
        fn no_payload_can_panic_the_parsers(text in jsonish()) {
            let _ = serde_json::from_str::<League>(&text);
            let _ = serde_json::from_str::<Draft>(&text);
            let _ = serde_json::from_str::<Pick>(&text);
            let _ = serde_json::from_str::<PlayerMeta>(&text);
            let _ = serde_json::from_str::<ProjectionRow>(&text);
            let _ = serde_json::from_str::<Vec<ProjectionRow>>(&text);
        }

        /// Same, for arbitrary bytes that may not even be valid UTF-8.
        #[test]
        fn no_byte_string_can_panic_the_parsers(
            bytes in prop::collection::vec(any::<u8>(), 0..256)
        ) {
            let _ = serde_json::from_slice::<League>(&bytes);
            let _ = serde_json::from_slice::<Draft>(&bytes);
            let _ = serde_json::from_slice::<Vec<ProjectionRow>>(&bytes);
        }
    }
}
