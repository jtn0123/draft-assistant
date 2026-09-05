//! What separates the three modes, and what the strategy layer is worth.
//!
//! Helpers come from `recommend_tests.rs`, which is the other half of this
//! module's tests — split only to stay inside the file-length cap.

use super::tests::{of_mode, player, recs, roster, slots};
use super::*;
use crate::board::AvailablePlayer;
use crate::roster::RosterRules;

// ---------- the three modes ----------

#[test]
fn every_mode_the_panel_shows_is_produced() {
    let available = vec![player("rb1", "RB", 40.0), player("wr1", "WR", 35.0)];
    let mine = roster(&["QB"]);
    let recs = recs(
        &available,
        Some(&mine),
        &RosterRules::new(&slots()),
        3,
        15,
        30,
    );
    let modes: Vec<&str> = recs.iter().map(|r| r.mode.as_str()).collect();
    assert_eq!(modes, vec!["balanced", "safe", "upside"]);
    assert_eq!(MODES.len(), modes.len());
}

#[test]
fn upside_takes_the_swingier_of_two_identical_players() {
    // Same VORP, same tier, same slot need. One is the same player every week
    // and the other is boom-or-bust; only upside is buying the second.
    let mut steady = player("steady", "WR", 40.0);
    steady.player.weekly_cv = Some(0.25);
    let mut swingy = player("swingy", "WR", 40.0);
    swingy.player.weekly_cv = Some(0.95);
    let available = vec![steady, swingy];
    let mine = roster(&["QB"]);
    let recs = recs(
        &available,
        Some(&mine),
        &RosterRules::new(&slots()),
        3,
        15,
        30,
    );
    assert_eq!(of_mode(&recs, "upside").player_id, "swingy", "{recs:?}");
    // Balanced has no reason to prefer either, so it keeps board order.
    assert_eq!(of_mode(&recs, "balanced").player_id, "steady", "{recs:?}");
}

#[test]
fn upside_will_take_a_bench_body_late_where_balanced_will_not() {
    // Last two rounds, both flex slots long since filled. Balanced marks the
    // depth pick down and takes the safe body; upside pays the smaller
    // penalty and takes the one with a ceiling.
    let mut dull = player("dull", "WR", 6.0);
    dull.player.weekly_cv = Some(0.2);
    let mut lottery = player("lottery", "WR", 2.0);
    lottery.player.weekly_cv = Some(1.1);
    let available = vec![dull, lottery];
    let mut mine = roster(&["QB", "RB", "RB", "WR", "WR", "WR", "TE"]);
    mine.open_starters = vec![];
    let recs = recs(
        &available,
        Some(&mine),
        &RosterRules::new(&slots()),
        14,
        15,
        170,
    );
    assert_eq!(of_mode(&recs, "upside").player_id, "lottery", "{recs:?}");
    assert_eq!(of_mode(&recs, "balanced").player_id, "dull", "{recs:?}");
}

// ---------- the market ----------

#[test]
fn safe_mode_penalises_a_reach_and_not_a_bargain() {
    // Both have the same VORP. One goes at pick 20 by ADP and would be a
    // fifty-pick reach here; the other has already fallen past his ADP.
    let mut reach = player("reach", "WR", 30.0);
    reach.player.adp = Some(70.0);
    let mut bargain = player("bargain", "WR", 30.0);
    bargain.player.adp = Some(10.0);
    let available = vec![reach, bargain];
    let mine = roster(&["QB"]);
    let recs = recs(
        &available,
        Some(&mine),
        &RosterRules::new(&slots()),
        2,
        15,
        20,
    );
    let safe = of_mode(&recs, "safe");
    assert_eq!(safe.player_id, "bargain", "{recs:?}");
    // And says so: the old form's "reach" was the board disagreeing with the
    // market, so it fired on the bargain and never on the reach.
    let picked_reach = recs
        .iter()
        .find(|r| r.player_id == "reach")
        .map(|r| r.reasons.join(" "));
    assert!(
        picked_reach.is_none() || !picked_reach.unwrap().contains("a reach"),
        "{recs:?}"
    );
}

// ---------- injuries ----------

fn with_tag(id: &str, tag: Option<&str>) -> AvailablePlayer {
    let mut p = player(id, "WR", 30.0);
    p.player.injury_status = tag.map(|t| t.to_string());
    p
}

#[test]
fn injuries_are_priced_by_what_the_tag_means() {
    let mine = roster(&["QB"]);
    let rules = RosterRules::new(&slots());
    // Out beats Questionable beats healthy, in that order, downwards.
    let available = vec![
        with_tag("healthy", None),
        with_tag("questionable", Some("Questionable")),
        with_tag("out", Some("Out")),
    ];
    let picked = recs(&available, Some(&mine), &rules, 3, 15, 30);
    // Balanced now reads injuries at all — it used to ignore them entirely.
    assert_eq!(
        of_mode(&picked, "balanced").player_id,
        "healthy",
        "{picked:?}"
    );
    assert_eq!(of_mode(&picked, "safe").player_id, "healthy", "{picked:?}");

    // A practice tag is worth about two points, not fifteen: it must not be
    // able to demote a man who is twenty VORP better.
    let mut better = with_tag("better", Some("Questionable"));
    better.player.vorp = 50.0;
    better.player.points = 200.0;
    let available = vec![better, with_tag("worse", None)];
    let picked = recs(&available, Some(&mine), &rules, 3, 15, 30);
    assert_eq!(of_mode(&picked, "safe").player_id, "better", "{picked:?}");

    // "Out" is a different matter and safe mode weights it harder still.
    let mut hurt = with_tag("hurt", Some("Out"));
    hurt.player.vorp = 42.0;
    hurt.player.points = 192.0;
    let available = vec![hurt, with_tag("fit", None)];
    let picked = recs(&available, Some(&mine), &rules, 3, 15, 30);
    assert_eq!(of_mode(&picked, "safe").player_id, "fit", "{picked:?}");
}

#[test]
fn a_practice_tag_is_ignored_before_the_draft_starts() {
    // In August "Questionable" is left over from a practice report and says
    // nothing about week 1. Nobody should be marked down for it.
    let available = vec![with_tag("q", Some("Questionable"))];
    let mine = roster(&["QB"]);
    let rules = RosterRules::new(&slots());
    let mut inputs = RecommendInputs::new(&available, Some(&mine), &rules, 1, 15, 1, 12);
    let during = recommend(&inputs);
    inputs.pre_draft = true;
    let before = recommend(&inputs);
    assert!(
        of_mode(&before, "safe").score > of_mode(&during, "safe").score,
        "{before:?} vs {during:?}"
    );
    assert!(of_mode(&before, "safe")
        .reasons
        .iter()
        .all(|r| !r.contains("injury")));

    // An "Out" tag is not a practice report and still counts, and the line
    // says which tag it was rather than the word "injury": what a drafter
    // needs off the card is how much of the season is gone.
    let available = vec![with_tag("o", Some("Out"))];
    let mut inputs = RecommendInputs::new(&available, Some(&mine), &rules, 1, 15, 1, 12);
    inputs.pre_draft = true;
    assert!(of_mode(&recommend(&inputs), "safe")
        .reasons
        .iter()
        .any(|r| r.contains("tagged Out")));
}

// ---------- the strategy layer ----------

#[test]
fn a_run_on_a_thin_position_is_worth_chasing() {
    let available = vec![player("wr1", "WR", 30.0), player("rb1", "RB", 30.0)];
    let mine = roster(&["QB"]);
    let rules = RosterRules::new(&slots());
    let run = PositionRun {
        position: "RB".into(),
        count: 4,
        window: 6,
    };
    let mut inputs = RecommendInputs::new(&available, Some(&mine), &rules, 3, 15, 30, 12);
    let quiet = recommend(&inputs);
    inputs.position_run = Some(&run);
    let running = recommend(&inputs);
    assert_eq!(of_mode(&quiet, "balanced").player_id, "wr1", "{quiet:?}");
    assert_eq!(
        of_mode(&running, "balanced").player_id,
        "rb1",
        "{running:?}"
    );
    assert!(of_mode(&running, "balanced")
        .reasons
        .iter()
        .any(|r| r.contains("run on RB")));
}

#[test]
fn stacking_byes_on_the_starting_lineup_costs_something() {
    let mut clash = player("clash", "WR", 31.0);
    clash.player.bye_week = Some(9);
    let mut clear = player("clear", "WR", 30.0);
    clear.player.bye_week = Some(11);
    let available = vec![clash, clear];
    let mine = roster(&["QB"]);
    let rules = RosterRules::new(&slots());
    let mut byes = HashMap::new();
    byes.insert(9u32, 3u32);
    let mut inputs = RecommendInputs::new(&available, Some(&mine), &rules, 3, 15, 30, 12);
    assert_eq!(of_mode(&recommend(&inputs), "balanced").player_id, "clash");
    inputs.my_byes = &byes;
    let stacked = recommend(&inputs);
    assert_eq!(
        of_mode(&stacked, "balanced").player_id,
        "clear",
        "{stacked:?}"
    );
}

#[test]
fn the_back_behind_my_own_back_is_worth_a_little_more() {
    let mut handcuff = player("handcuff", "RB", 8.0);
    handcuff.player.team = Some("DET".into());
    let mut stranger = player("stranger", "RB", 9.0);
    stranger.player.team = Some("KC".into());
    let available = vec![handcuff, stranger];
    let mut mine = roster(&["QB", "RB", "WR", "TE"]);
    mine.players[1].team = Some("DET".into());
    let recs = recs(
        &available,
        Some(&mine),
        &RosterRules::new(&slots()),
        9,
        15,
        110,
    );
    assert_eq!(of_mode(&recs, "balanced").player_id, "handcuff", "{recs:?}");
    assert!(of_mode(&recs, "balanced")
        .reasons
        .iter()
        .any(|r| r.contains("handcuffs the DET back")));
}

// ---------- depth, and how the reasons are ordered ----------

#[test]
fn the_depth_penalty_grows_with_the_pile() {
    // One spare receiver is insurance; the fifth is a wasted roster spot.
    let rules = RosterRules::new(&slots());
    let score_with = |already: &[&str]| {
        let mut mine = roster(already);
        mine.open_starters = vec![];
        let available = vec![player("wr", "WR", 10.0)];
        recs(&available, Some(&mine), &rules, 12, 15, 140)
            .iter()
            .find(|r| r.mode == "balanced")
            .expect("a balanced pick")
            .score
    };
    let one = score_with(&["WR"]);
    let four = score_with(&["WR", "WR", "WR", "WR"]);
    assert!(one > four, "{one} vs {four}");
}

#[test]
fn the_reasons_are_ordered_by_what_they_were_worth() {
    // A late body with barely any VORP: the "one injury from an empty slot"
    // bonus is what actually picked him, and the panel only shows two lines,
    // so that has to come before the VORP boilerplate.
    let available = vec![player("wr", "WR", 2.0)];
    let mut mine = roster(&["QB", "RB", "RB", "TE"]);
    // No starting slot reported open, so the thin-room bonus is the only
    // term paying for the empty receiver room and it is what picked him.
    mine.open_starters = vec![];
    let recs = recs(
        &available,
        Some(&mine),
        &RosterRules::new(&slots()),
        12,
        15,
        140,
    );
    let reasons = &of_mode(&recs, "balanced").reasons;
    assert!(
        !reasons[0].contains("VORP"),
        "the boilerplate led anyway: {reasons:?}"
    );
    assert!(
        reasons[0].contains("empty slot") || reasons[0].contains("starter slot"),
        "{reasons:?}"
    );
}
