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

#[test]
fn a_doubtful_tag_before_the_draft_is_last_seasons_news() {
    // The pre-draft gate dropped "Questionable" and left "Doubtful" standing,
    // so an August practice report left over from January took nine points
    // off a safe-mode card.
    let available = vec![
        tagged("doubtful", "WR", 40.0, Some("Doubtful")),
        tagged("fit", "WR", 40.0, None),
    ];
    let mine = roster(&["QB", "RB"]);
    let rules = RosterRules::new(&slots());
    let mut inputs = RecommendInputs::new(&available, Some(&mine), &rules, 1, 15, 1, 12);
    inputs.pre_draft = true;
    let ctx = context(&inputs, HashMap::from([("QB", 1), ("RB", 1)]));
    let hurt = score_candidate(&ctx, &available[0], Mode::Safe).expect("a WR");
    let fit = score_candidate(&ctx, &available[1], Mode::Safe).expect("a WR");
    assert!(
        (hurt.total - fit.total).abs() < 1e-9,
        "a pre-draft practice tag cost {}",
        fit.total - hurt.total
    );
    // Once the draft is live the same tag counts again.
    let inputs = RecommendInputs::new(&available, Some(&mine), &rules, 1, 15, 1, 12);
    let ctx = context(&inputs, HashMap::from([("QB", 1), ("RB", 1)]));
    let live = score_candidate(&ctx, &available[0], Mode::Safe).expect("a WR");
    assert!(live.total < fit.total, "{} vs {}", live.total, fit.total);
}

#[test]
fn being_out_for_a_week_costs_a_week_and_not_a_quarter_of_the_season() {
    // "Out" is a ruling for one game. Priced at a quarter of the season it
    // cost a 90-VORP back thirteen points, more than any other term on his
    // card, for missing one Sunday out of eighteen.
    let available = vec![
        tagged("out", "RB", 90.0, Some("Out")),
        tagged("fit", "RB", 90.0, None),
    ];
    let mine = roster(&["QB", "WR"]);
    let rules = RosterRules::new(&slots());
    let inputs = RecommendInputs::new(&available, Some(&mine), &rules, 4, 15, 40, 12);
    let ctx = context(&inputs, HashMap::from([("QB", 1), ("WR", 1)]));
    let out = score_candidate(&ctx, &available[0], Mode::Balanced).expect("an RB");
    let fit = score_candidate(&ctx, &available[1], Mode::Balanced).expect("an RB");
    let cost = fit.total - out.total;
    assert!(
        (0.0..=5.0).contains(&cost),
        "one week out of eighteen cost {cost}"
    );

    // And it is a share of what is left: with two weeks to play, the same tag
    // takes a ninth of the season rather than an eighteenth.
    let mut inputs = RecommendInputs::new(&available, Some(&mine), &rules, 4, 15, 40, 12);
    inputs.weeks_left = 2;
    let ctx = context(&inputs, HashMap::from([("QB", 1), ("WR", 1)]));
    let late = score_candidate(&ctx, &available[0], Mode::Balanced).expect("an RB");
    assert!(
        fit.total - late.total > cost,
        "a week is a bigger share of two weeks: {} vs {cost}",
        fit.total - late.total
    );
}
