//! Where a flex slot's demand goes, how the card says so, and what the
//! market's distance is worth.

use super::league_tests::context;
use super::score::{score_candidate, starting_demand};
use super::tests::{entry, player, slots};
use super::*;
use crate::board::AvailablePlayer;

fn rules(slots: &[&str]) -> RosterRules {
    RosterRules::new(
        &slots
            .iter()
            .map(|slot| (*slot).to_string())
            .collect::<Vec<_>>(),
    )
}

fn roster_with(positions: &[&str], open: &[(&str, u32)]) -> TeamRoster {
    TeamRoster {
        slot: 2,
        display_name: None,
        players: positions
            .iter()
            .enumerate()
            .map(|(i, p)| entry(p, i as u32 + 1))
            .collect(),
        open_starters: open
            .iter()
            .map(|(slot, n)| ((*slot).to_string(), *n))
            .collect(),
    }
}

/// A board deep enough for a twelve-team league to allocate against: forty
/// bodies a position, quarterbacks scoring the way quarterbacks do.
fn deep_board() -> Vec<AvailablePlayer> {
    let mut board = Vec::new();
    for (position, top) in [("QB", 380.0), ("RB", 300.0), ("WR", 295.0), ("TE", 220.0)] {
        for i in 0..40 {
            let points = top - 4.0 * f64::from(i);
            let mut p = player(&format!("{position}{i}"), position, points - 150.0);
            p.player.points = points;
            p.player.adp = None;
            board.push(p);
        }
    }
    board
}

// ---------- where a superflex slot's demand goes ----------

#[test]
fn superflex_demand_goes_to_quarterbacks_not_a_quarter_to_each_position() {
    // The old split gave every eligible position 1/4 of a SUPER_FLEX, so a
    // roster with one quarterback in a league that starts two read as a
    // quarter of a body short and took a third running back instead. The
    // replacement model never believed that: it hands the slot to whoever
    // values it most, which in a superflex league is the quarterback.
    let board = deep_board();
    let superflex = rules(&[
        "QB",
        "RB",
        "RB",
        "WR",
        "WR",
        "TE",
        "FLEX",
        "SUPER_FLEX",
        "BN",
    ]);
    let demand = starting_demand(&board, &superflex, 12);
    assert!(
        demand["QB"] > 1.9,
        "a superflex league starts {} quarterbacks",
        demand["QB"]
    );
    // One quarterback rostered against two running backs: the quarterback
    // room is the bigger hole, and the need model has to say so.
    let qb_short = demand["QB"] - 1.0;
    let rb_short = (demand["RB"] - 2.0).max(0.0);
    assert!(
        qb_short > rb_short,
        "QB short {qb_short} vs RB short {rb_short} ({demand:?})"
    );

    // And end to end, at identical VORP with no starting slot open to
    // confuse the comparison.
    let mine = roster_with(&["QB", "RB", "RB", "WR", "WR", "TE"], &[]);
    let candidates = [player("qb_x", "QB", 40.0), player("rb_x", "RB", 40.0)];
    let mut board = board;
    board.extend(candidates.iter().cloned());
    let inputs = RecommendInputs::new(&board, Some(&mine), &superflex, 6, 15, 60, 12);
    let ctx = context(
        &inputs,
        HashMap::from([("QB", 1), ("RB", 2), ("WR", 2), ("TE", 1)]),
    );
    let qb = score_candidate(&ctx, &candidates[0], Mode::Balanced).expect("a QB");
    let rb = score_candidate(&ctx, &candidates[1], Mode::Balanced).expect("an RB");
    assert!(qb.total > rb.total, "QB {} vs RB {}", qb.total, rb.total);
}

#[test]
fn a_one_quarterback_league_does_not_want_a_second_one() {
    let board = deep_board();
    let one_qb = rules(&["QB", "RB", "RB", "WR", "WR", "TE", "FLEX", "BN"]);
    let demand = starting_demand(&board, &one_qb, 12);
    assert!(
        (demand["QB"] - 1.0).abs() < 1e-9,
        "a one-QB league starts {}",
        demand["QB"]
    );
    // The flex is a running back or a receiver question, and it is allocated
    // whole rather than a third each.
    let flex = demand["RB"] + demand["WR"] + demand.get("TE").copied().unwrap_or(0.0) - 5.0;
    assert!((flex - 1.0).abs() < 1e-9, "the flex came to {flex}");
}

// ---------- the phrase next to the number ----------

#[test]
fn the_starters_phrase_says_what_the_term_is_worth() {
    // "about 2 with flex" for 1.25 made two claims the score does not: that
    // the league starts two, and that the term beside it was worth two.
    let flex = rules(&["QB", "RB", "RB", "WR", "WR", "TE", "FLEX", "BN"]);
    let phrase = |demand: f64| super::score::starters_phrase(&flex, "RB", demand);
    assert_eq!(phrase(1.0), "1 starter");
    assert_eq!(phrase(1.25), "1 starter plus a share of the flex");
    assert_eq!(phrase(1.5), "1 starter plus a share of the flex");
    assert_eq!(phrase(2.0), "2 starters");
    assert_eq!(phrase(2.33), "2 starters plus a share of the flex");

    let superflex = rules(&["QB", "RB", "WR", "TE", "SUPER_FLEX", "BN"]);
    assert_eq!(
        super::score::starters_phrase(&superflex, "QB", 1.75),
        "1 starter plus a share of the superflex"
    );
    assert_eq!(
        super::score::starters_phrase(&superflex, "QB", 2.0),
        "2 starters"
    );
}

// ---------- the market's distance ----------

/// The balanced score of one receiver with a given ADP, at a given market
/// pick. Nothing else about the card changes between calls.
fn score_with(adp: f64, market_pick: u32) -> f64 {
    let mut available = vec![player("wr1", "WR", 40.0)];
    available[0].player.adp = Some(adp);
    let mine = roster_with(&["QB", "RB", "WR", "TE"], &[]);
    let standard = RosterRules::new(&slots());
    let inputs = RecommendInputs::new(&available, Some(&mine), &standard, 3, 15, market_pick, 12);
    let ctx = context(
        &inputs,
        HashMap::from([("QB", 1), ("RB", 1), ("WR", 1), ("TE", 1)]),
    );
    score_candidate(&ctx, &available[0], Mode::Balanced)
        .expect("a receiver")
        .total
}

#[test]
fn falling_pays_for_the_distance_the_reason_quotes() {
    // A flat +5 paid the same for nine picks past ADP and for sixty-two,
    // under a line that quoted the real number both times.
    let level = score_with(20.0, 20);
    let short_fall = score_with(20.0, 29) - level;
    let long_fall = score_with(20.0, 82) - level;
    assert!(short_fall > 0.0, "nine picks past ADP paid {short_fall}");
    assert!(
        long_fall > short_fall,
        "sixty-two picks past ADP paid {long_fall}, nine paid {short_fall}"
    );
    // And capped: past a few rounds the market is not saying "bargain", it is
    // saying something this board has not heard about.
    assert!(long_fall <= 8.0 + 1e-9, "the cap let through {long_fall}");
    assert!(score_with(20.0, 400) - level <= 8.0 + 1e-9);
}

#[test]
fn a_reach_past_the_market_is_priced_by_its_distance_too() {
    // Measured at one pick, against three ADPs: inside the band, forty picks
    // early, and two hundred early.
    let level = score_with(30.0, 30);
    assert!(
        (score_with(50.0, 30) - level).abs() < 1e-9,
        "twenty picks early is inside the band and must cost nothing"
    );
    let mild = level - score_with(70.0, 30);
    let wild = level - score_with(230.0, 30);
    assert!(mild > 0.0, "forty picks early cost {mild}");
    assert!(wild > mild, "mild {mild} vs wild {wild}");
    assert!(wild <= 6.0 + 1e-9, "uncapped: {wild}");
}

// ---------- one hole, one bonus ----------

#[test]
fn the_thin_room_bonus_defers_to_the_open_slot_that_already_paid_for_it() {
    // Both terms fired on the same empty receiver room past round eight: 12
    // times the need pressure for the open slot, and 20 more for the room
    // being one injury from empty. One hole, paid for twice.
    let available = vec![player("wr", "WR", 10.0)];
    let standard = RosterRules::new(&slots());
    let reasons_when = |open: &[(&str, u32)]| {
        let mine = roster_with(&["QB", "RB", "RB", "WR", "TE"], open);
        let inputs = RecommendInputs::new(&available, Some(&mine), &standard, 10, 15, 110, 12);
        let ctx = context(
            &inputs,
            HashMap::from([("QB", 1), ("RB", 2), ("WR", 1), ("TE", 1)]),
        );
        score_candidate(&ctx, &available[0], Mode::Balanced)
            .expect("a receiver")
            .into_reasons()
    };
    let both = reasons_when(&[("WR", 1)]);
    assert!(both.iter().any(|r| r.contains("starter slot")), "{both:?}");
    assert!(
        !both.iter().any(|r| r.contains("one injury from an empty")),
        "the same hole was paid for twice: {both:?}"
    );
    // With no starting slot open, the thin-room warning is the only one
    // saying it, so it still fires.
    let thin_only = reasons_when(&[]);
    assert!(
        thin_only
            .iter()
            .any(|r| r.contains("one injury from an empty")),
        "{thin_only:?}"
    );
}
