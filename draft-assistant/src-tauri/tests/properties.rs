//! Property-based tests over the draft math.
//!
//! The unit tests pin known cases against the real league document; these pin
//! the *invariants* that must hold for every league shape Sleeper can report,
//! including shapes nobody has drafted yet. Written after a degenerate
//! `teams: 0` payload was found to panic `build_view`.

use draft_assistant_lib::draft::{picks_for_slot, slot_for_pick, survival_probability, DraftOrder};
use draft_assistant_lib::roster::RosterRules;
use draft_assistant_lib::scoring::{base_points, norm_cdf};
use proptest::prelude::*;
use std::collections::{HashMap, HashSet};

/// Real leagues run 2-32 teams and 1-30 rounds; go wider than that on purpose.
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

proptest! {
    /// The clock must always land on a real team.
    #[test]
    fn slot_for_pick_is_always_a_real_slot(
        teams in teams(),
        rounds in rounds(),
        offset in 0u32..1200,
        order in order(),
    ) {
        let pick = (offset % (teams * rounds)) + 1;
        let slot = slot_for_pick(pick, teams, order);
        prop_assert!((1..=teams).contains(&slot), "slot {slot} outside 1..={teams}");
    }

    /// Snake: a slot's pick in an odd round and the next even round are
    /// mirrored, so the two slot numbers sum to teams + 1.
    #[test]
    fn snake_rounds_mirror_each_other(teams in 2u32..=40, idx in 0u32..40) {
        let order = DraftOrder::SNAKE;
        let idx = idx % teams;
        let first = slot_for_pick(idx + 1, teams, order);
        let second = slot_for_pick(teams + idx + 1, teams, order);
        prop_assert_eq!(first + second, teams + 1);
    }

    /// Every slot drafts exactly once per round, and the per-slot pick lists
    /// partition the whole draft with no gaps or overlaps.
    #[test]
    fn picks_for_slot_partitions_the_draft(
        teams in teams(),
        rounds in rounds(),
        order in order(),
    ) {
        let mut all = Vec::new();
        for slot in 1..=teams {
            let picks = picks_for_slot(slot, teams, rounds, order);
            prop_assert_eq!(
                picks.len(),
                rounds as usize,
                "slot {} got {} picks, expected {}", slot, picks.len(), rounds
            );
            // Every pick this slot owns must map back to this slot.
            for &pick in &picks {
                prop_assert_eq!(slot_for_pick(pick, teams, order), slot);
            }
            all.extend(picks);
        }
        let unique: HashSet<u32> = all.iter().copied().collect();
        prop_assert_eq!(unique.len(), all.len(), "a pick belongs to two slots");
        prop_assert_eq!(all.len(), (teams * rounds) as usize);
    }

    /// Degenerate settings must return a number, not panic. `overflow-checks`
    /// is on in release, so an underflow here would kill the app mid-draft.
    #[test]
    fn slot_for_pick_survives_degenerate_settings(
        pick in 0u32..100,
        teams in 0u32..3,
        order in order(),
    ) {
        let slot = slot_for_pick(pick, teams, order);
        prop_assert!(slot >= 1);
    }

    /// A probability must be a probability, for any ADP and any pick.
    #[test]
    fn survival_is_always_a_probability(
        adp in -50.0f64..2000.0,
        now_pick in 0u32..1000,
        at_pick in 0u32..1000,
    ) {
        let p = survival_probability(adp, now_pick, at_pick);
        prop_assert!(p.is_finite(), "survival was {p}");
        prop_assert!((0.0..=1.0).contains(&p), "survival {p} outside [0,1]");
    }

    /// Later picks can only make a player less likely to still be there.
    #[test]
    fn survival_never_increases_with_later_picks(
        adp in 1.0f64..300.0,
        now in 1u32..300,
        ahead in 0u32..100,
    ) {
        let earlier = survival_probability(adp, now, now + ahead);
        let later = survival_probability(adp, now, now + ahead + 1);
        prop_assert!(
            later <= earlier + f64::EPSILON,
            "survival rose from {earlier} to {later}"
        );
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

    /// Scoring is a dot product over the league's own key space: unknown stat
    /// keys contribute nothing, and the result is always a real number.
    #[test]
    fn scoring_ignores_unscored_keys_and_stays_finite(
        pass_yd in 0.0f64..6000.0,
        rec in 0.0f64..200.0,
        junk in -1e6f64..1e6,
    ) {
        let mut stats = HashMap::new();
        stats.insert("pass_yd".to_string(), pass_yd);
        stats.insert("rec".to_string(), rec);
        let mut scoring = HashMap::new();
        scoring.insert("pass_yd".to_string(), 0.04);
        scoring.insert("rec".to_string(), 1.0);

        let base = base_points(&stats, &scoring);
        prop_assert!(base.is_finite());

        // A stat the league does not score must not move the total.
        stats.insert("not_a_scored_stat".to_string(), junk);
        prop_assert!((base_points(&stats, &scoring) - base).abs() < 1e-9);
    }

    /// Whatever roster shape a league declares, the positions we agree to draft
    /// must all be fillable by some slot in it.
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
        let owned: Vec<String> = slots.iter().map(|s| s.to_string()).collect();
        let rules = RosterRules::new(&owned);
        for position in rules.draftable_positions() {
            prop_assert!(
                owned.iter().any(|slot| RosterRules::can_fill(slot, &position)),
                "{position} is draftable but no slot can hold it"
            );
        }
    }

    /// Bench and IR slots never make a position draftable on their own.
    #[test]
    fn non_starting_slots_alone_draft_nothing(
        slots in prop::collection::vec(prop::sample::select(vec!["BN", "IR", "TAXI"]), 1..10)
    ) {
        let owned: Vec<String> = slots.iter().map(|s| s.to_string()).collect();
        prop_assert!(RosterRules::new(&owned).draftable_positions().is_empty());
    }
}

/// Randomized parsing robustness.
///
/// This is the coverage the `sleeper_payloads` fuzz target is meant to give.
/// That target builds but its libFuzzer runtime does not execute on macOS 27
/// with the pinned cargo-fuzz (see `src-tauri/fuzz/README.md`), so the same
/// invariant is asserted here, where it actually runs on every `bun run test`.
mod parsing_robustness {
    use draft_assistant_lib::sleeper::{Draft, League, Pick, PlayerMeta, ProjectionRow};
    use proptest::prelude::*;

    /// Strings biased toward JSON: structural characters, digits, and the key
    /// names the real payloads use, so the generator reaches the parser rather
    /// than bouncing off the first byte.
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
                "NaN",
                "Infinity",
            ]),
            0..40,
        )
        .prop_map(|parts| parts.concat())
    }

    proptest! {
        #![proptest_config(ProptestConfig::with_cases(2048))]

        /// Deserializing hostile input must return Ok or Err — never panic.
        /// These types come straight off an undocumented API.
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
