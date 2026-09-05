//! What an injury tag is allowed to do to a card.
//!
//! The failure these prevent: a flat 25-point tag against a VORP term worth
//! 0.6 of a player's whole value over replacement, which left a running back
//! on injured reserve at the top of the board all three ways.

use super::league_tests::context;
use super::score::score_candidate;
use super::tests::{of_mode, player, recs, roster, slots};
use super::*;
use crate::board::AvailablePlayer;

fn tagged(id: &str, position: &str, vorp: f64, tag: Option<&str>) -> AvailablePlayer {
    let mut p = player(id, position, vorp);
    p.player.injury_status = tag.map(|t| t.to_string());
    p
}

#[test]
fn a_back_on_injured_reserve_loses_to_a_healthy_lesser_back_in_every_mode() {
    // The whole failure in one board: an elite back who will not play, and an
    // ordinary one who will. A flat 25 off a 90-VORP card still left the man
    // on IR ahead by nearly thirty points.
    let available = vec![
        tagged("ir_rb1", "RB", 90.0, Some("IR")),
        tagged("healthy_rb2", "RB", 60.0, None),
    ];
    let mine = roster(&["QB", "WR"]);
    let picked = recs(
        &available,
        Some(&mine),
        &RosterRules::new(&slots()),
        4,
        15,
        40,
    );
    for mode in MODES {
        assert_eq!(
            of_mode(&picked, mode).player_id,
            "healthy_rb2",
            "{mode}: {picked:?}"
        );
    }
}

#[test]
fn a_questionable_receiver_barely_moves() {
    // A practice-report tag is about Sunday, not about the season, so it has
    // to stay small enough that a better player keeps his place.
    let available = vec![
        tagged("q", "WR", 40.0, Some("Questionable")),
        tagged("fit", "WR", 40.0, None),
    ];
    let mine = roster(&["QB", "RB"]);
    let rules = RosterRules::new(&slots());
    let inputs = RecommendInputs::new(&available, Some(&mine), &rules, 4, 15, 40, 12);
    let ctx = context(&inputs, HashMap::from([("QB", 1), ("RB", 1)]));
    let hurt = score_candidate(&ctx, &available[0], Mode::Balanced).expect("a WR");
    let fit = score_candidate(&ctx, &available[1], Mode::Balanced).expect("a WR");
    let gap = fit.total - hurt.total;
    assert!((0.0..=4.0).contains(&gap), "a practice tag cost {gap}");

    // The same two players with a season-ending tag instead: now it is the
    // whole card, because that is what the tag takes away.
    let season = vec![tagged("ir", "WR", 40.0, Some("IR"))];
    let inputs = RecommendInputs::new(&season, Some(&mine), &rules, 4, 15, 40, 12);
    let ctx = context(&inputs, HashMap::from([("QB", 1), ("RB", 1)]));
    let out = score_candidate(&ctx, &season[0], Mode::Balanced).expect("a WR");
    assert!(
        fit.total - out.total > 20.0,
        "IR cost only {}",
        fit.total - out.total
    );
}

#[test]
fn the_reason_names_the_tag_and_what_it_costs() {
    let available = vec![tagged("ir", "RB", 40.0, Some("IR"))];
    let mine = roster(&["QB", "WR"]);
    let rules = RosterRules::new(&slots());
    let inputs = RecommendInputs::new(&available, Some(&mine), &rules, 4, 15, 40, 12);
    let ctx = context(&inputs, HashMap::from([("QB", 1), ("WR", 1)]));
    let reasons = score_candidate(&ctx, &available[0], Mode::Balanced)
        .expect("a hurt RB is still a candidate")
        .into_reasons();
    assert!(
        reasons
            .iter()
            .any(|r| r == "on IR: most of the season gone"),
        "{reasons:?}"
    );
}

#[test]
fn the_reasons_still_add_up_to_the_score_with_a_tag_on_the_card() {
    let available = vec![
        tagged("ir", "RB", 40.0, Some("IR")),
        tagged("out", "WR", 30.0, Some("Out")),
        tagged("q", "TE", 20.0, Some("Questionable")),
    ];
    let mine = roster(&["QB", "WR"]);
    let rules = RosterRules::new(&slots());
    let inputs = RecommendInputs::new(&available, Some(&mine), &rules, 4, 15, 40, 12);
    let ctx = context(&inputs, HashMap::from([("QB", 1), ("WR", 1)]));
    for mode in [Mode::Balanced, Mode::Safe, Mode::Upside] {
        for candidate in &available {
            let Some(score) = score_candidate(&ctx, candidate, mode) else {
                continue;
            };
            let summed: f64 = score.weights().iter().sum();
            assert!(
                (summed - score.total).abs() < 1e-9,
                "{} in {mode:?}: reasons sum to {summed} but the score is {}",
                candidate.player.player_id,
                score.total
            );
        }
    }
}
