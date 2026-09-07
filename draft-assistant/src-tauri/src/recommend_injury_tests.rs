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

/// A league as the loader hands it over, with only the field that says which
/// week its season starts.
fn league_starting_in_week(start_week: Option<u32>) -> crate::sleeper::League {
    crate::sleeper::League {
        league_id: "league".into(),
        name: "league".into(),
        season: "2026".into(),
        status: "drafting".into(),
        total_rosters: 12,
        roster_positions: slots(),
        scoring_settings: HashMap::new(),
        draft_id: None,
        previous_league_id: None,
        settings: crate::sleeper::LeagueSettings {
            start_week,
            ..Default::default()
        },
    }
}

#[test]
fn a_league_that_does_not_say_when_it_starts_drafts_for_the_whole_season() {
    // An August draft, a mock draft with no league behind it, a Yahoo league
    // whose import carries no start week: all of them are the full season,
    // and none of them may be zero, because the one-week tag divides by it.
    assert_eq!(
        weeks_left(&league_starting_in_week(None)),
        crate::board::WEEKS
    );
    assert_eq!(
        weeks_left(&league_starting_in_week(Some(1))),
        crate::board::WEEKS
    );
    assert_eq!(
        weeks_left(&league_starting_in_week(Some(0))),
        crate::board::WEEKS
    );
    assert_eq!(weeks_left(&league_starting_in_week(Some(10))), 9);
    assert_eq!(weeks_left(&league_starting_in_week(Some(40))), 1);
}

#[test]
fn a_suspended_player_is_priced_as_missing_weeks_not_as_a_weekly_tag() {
    // "Suspended" was in the season screen's dictionary and not in the
    // recommender's, so it fell through to the unfamiliar-tag price: six
    // points, against the quarter of the season a "Sus" tag costs. The same
    // suspension, spelt two ways Sleeper both uses, has to cost the same.
    let available = vec![
        tagged("spelt_out", "RB", 90.0, Some("Suspended")),
        tagged("abbreviated", "RB", 90.0, Some("Sus")),
        tagged("fit", "RB", 90.0, None),
    ];
    let mine = roster(&["QB", "WR"]);
    let rules = RosterRules::new(&slots());
    let inputs = RecommendInputs::new(&available, Some(&mine), &rules, 4, 15, 40, 12);
    let ctx = context(&inputs, HashMap::from([("QB", 1), ("WR", 1)]));
    let spelt_out = score_candidate(&ctx, &available[0], Mode::Balanced).expect("an RB");
    let abbreviated = score_candidate(&ctx, &available[1], Mode::Balanced).expect("an RB");
    let fit = score_candidate(&ctx, &available[2], Mode::Balanced).expect("an RB");
    assert!(
        (spelt_out.total - abbreviated.total).abs() < 1e-9,
        "Suspended cost {} and Sus cost {}",
        fit.total - spelt_out.total,
        fit.total - abbreviated.total
    );
    assert!(
        fit.total - spelt_out.total > 10.0,
        "a suspension cost only {}",
        fit.total - spelt_out.total
    );
}

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
    // takes a ninth of the season rather than an eighteenth. Through the same
    // door the view uses, because a field set by hand in a test proved
    // nothing about production, where the view passed the full season to
    // every draft for a year.
    let mut inputs = RecommendInputs::new(&available, Some(&mine), &rules, 4, 15, 40, 12);
    inputs.weeks_left = weeks_left(&league_starting_in_week(Some(17)));
    assert_eq!(inputs.weeks_left, 2);
    let ctx = context(&inputs, HashMap::from([("QB", 1), ("WR", 1)]));
    let late = score_candidate(&ctx, &available[0], Mode::Balanced).expect("an RB");
    assert!(
        fit.total - late.total > cost,
        "a week is a bigger share of two weeks: {} vs {cost}",
        fit.total - late.total
    );
}

#[test]
fn yahoos_short_codes_cost_what_sleepers_words_cost() {
    // The failure this prevents: a Yahoo player's status is "Q", not
    // "Questionable", and it went to the recommender as it came. Every "Q"
    // fell through to the unfamiliar-tag price, six points, three times what
    // a practice report is worth, all night.
    let available = vec![
        tagged("q", "WR", 40.0, Some("Q")),
        tagged("questionable", "WR", 40.0, Some("Questionable")),
        tagged("o", "WR", 40.0, Some("O")),
        tagged("out", "WR", 40.0, Some("Out")),
        tagged("fit", "WR", 40.0, None),
    ];
    let mine = roster(&["QB", "RB"]);
    let rules = RosterRules::new(&slots());
    let inputs = RecommendInputs::new(&available, Some(&mine), &rules, 4, 15, 40, 12);
    let ctx = context(&inputs, HashMap::from([("QB", 1), ("RB", 1)]));
    let total = |index: usize| {
        score_candidate(&ctx, &available[index], Mode::Balanced)
            .expect("a WR")
            .total
    };
    assert!(
        (total(0) - total(1)).abs() < 1e-9,
        "Q {} vs Questionable {}",
        total(0),
        total(1)
    );
    assert!(
        (total(2) - total(3)).abs() < 1e-9,
        "O {} vs Out {}",
        total(2),
        total(3)
    );
    let q_cost = total(4) - total(0);
    assert!((0.0..=4.0).contains(&q_cost), "a Yahoo Q cost {q_cost}");
}

#[test]
fn a_camp_pup_tag_before_the_draft_is_early_weeks_not_a_lost_season() {
    // The failure this prevents: PUP is priced as season-ending, which is
    // right in October and wrong in August, when it is a training-camp
    // hamstring that most players come off at the roster cut. A first-round
    // back on camp PUP was docked his whole card, the same as a man on IR.
    let available = vec![
        tagged("pup", "RB", 40.0, Some("PUP")),
        tagged("pup_r", "RB", 40.0, Some("PUP-R")),
        tagged("ir", "RB", 40.0, Some("IR")),
        tagged("fit", "RB", 40.0, None),
    ];
    let mine = roster(&["QB", "WR"]);
    let rules = RosterRules::new(&slots());
    let mut inputs = RecommendInputs::new(&available, Some(&mine), &rules, 1, 15, 1, 12);
    inputs.pre_draft = true;
    let ctx = context(&inputs, HashMap::from([("QB", 1), ("WR", 1)]));
    let score =
        |index: usize| score_candidate(&ctx, &available[index], Mode::Balanced).expect("an RB");
    let fit = score(3).total;
    let pup_cost = fit - score(0).total;
    let ir_cost = fit - score(2).total;
    assert!(pup_cost > 0.0, "camp PUP cost nothing");
    assert!(
        pup_cost < ir_cost / 2.0,
        "camp PUP cost {pup_cost}, IR cost {ir_cost}"
    );
    assert!(
        (fit - score(1).total - pup_cost).abs() < 1e-9,
        "PUP-R in camp is the same list"
    );
    let reasons = score(0).into_reasons();
    assert!(
        reasons
            .iter()
            .any(|r| r == "on PUP in camp: may miss the early weeks"),
        "{reasons:?}"
    );

    // Once the season is under way the same tag is what the table says: a
    // reserve move in October is the year, and it costs what IR costs.
    let inputs = RecommendInputs::new(&available, Some(&mine), &rules, 1, 15, 1, 12);
    let ctx = context(&inputs, HashMap::from([("QB", 1), ("WR", 1)]));
    let live = |index: usize| {
        score_candidate(&ctx, &available[index], Mode::Balanced)
            .expect("an RB")
            .total
    };
    assert!(
        (live(0) - live(2)).abs() < 1e-9,
        "in season PUP {} vs IR {}",
        live(0),
        live(2)
    );
}
