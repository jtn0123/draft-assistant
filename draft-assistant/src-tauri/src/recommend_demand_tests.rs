//! Where a flex slot's demand goes, how the card says so, and what the
//! market's distance is worth.

use super::league_tests::context;
use super::score::{score_candidate, starting_demand};
use super::tests::{entry, player, slots};
use super::*;
use crate::board::AvailablePlayer;
use crate::draft::TeamRoster;

fn rules(slots: &[&str]) -> RosterRules {
    RosterRules::new(
        &slots
            .iter()
            .map(|slot| (*slot).to_string())
            .collect::<Vec<_>>(),
    )
}

/// The position-and-points pairs the demand allocator reads.
fn pool(board: &[AvailablePlayer]) -> Vec<(&str, f64)> {
    board
        .iter()
        .map(|a| (a.player.position.as_str(), a.player.points))
        .collect()
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

/// A roster whose open starting slots are the ones the roster rules actually
/// work out, rather than a list written by hand in the test. Which slot is
/// left open is half of what is under test here.
fn roster_via_rules(rules: &RosterRules, positions: &[&str]) -> TeamRoster {
    let open = rules.open_starting_slots(positions.iter().copied());
    TeamRoster {
        slot: 2,
        display_name: None,
        players: positions
            .iter()
            .enumerate()
            .map(|(i, p)| entry(p, i as u32 + 1))
            .collect(),
        open_starters: open,
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
    let demand = starting_demand(pool(&board), &superflex, 12);
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
    let demand = starting_demand(pool(&board), &one_qb, 12);
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

// ---------- whose hole an open flex slot is ----------

#[test]
fn an_open_superflex_slot_is_the_quarterbacks_hole_and_not_the_fourth_receivers() {
    // One SUPER_FLEX left open on a roster with one quarterback and three
    // receivers. Both candidates could legally go in it, so both were paid
    // the same eight points for "an open SUPER_FLEX slot" and the pick fell
    // to whoever had the bigger VORP, which at that point on the board is
    // always the receiver.
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
    let mine = roster_via_rules(&superflex, &["QB", "RB", "RB", "WR", "WR", "WR", "TE"]);
    assert_eq!(
        mine.open_starters,
        vec![("SUPER_FLEX".to_string(), 1)],
        "the roster rules have to leave the superflex open for this to be the question"
    );

    let candidates = [player("qb2", "QB", 40.0), player("wr4", "WR", 40.0)];
    let mut board = deep_board();
    board.extend(candidates.iter().cloned());
    let inputs = RecommendInputs::new(&board, Some(&mine), &superflex, 6, 15, 60, 12);
    let ctx = context(
        &inputs,
        HashMap::from([("QB", 1), ("RB", 2), ("WR", 3), ("TE", 1)]),
    );
    let qb = score_candidate(&ctx, &candidates[0], Mode::Balanced).expect("a QB");
    let wr = score_candidate(&ctx, &candidates[1], Mode::Balanced).expect("a WR");
    assert!(qb.total > wr.total, "QB {} vs WR {}", qb.total, wr.total);
    let said = qb.into_reasons();
    assert!(
        said.iter()
            .any(|r| r.contains("SUPER_FLEX slot is your QB")),
        "{said:?}"
    );
    let heard = wr.into_reasons();
    assert!(
        heard.iter().any(|r| r == "fills an open SUPER_FLEX slot"),
        "{heard:?}"
    );
}

// ---------- what the league starts in a slot of its own ----------

/// A board where tight ends outscore receivers, which is what a TE-premium
/// scoring table does to one.
fn te_premium_board() -> Vec<AvailablePlayer> {
    let mut board = Vec::new();
    for (position, top) in [("QB", 380.0), ("RB", 300.0), ("WR", 290.0), ("TE", 400.0)] {
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

#[test]
fn a_second_tight_end_is_not_a_backup_where_the_flex_allocation_wants_one() {
    // Counting slots by name saw one slot spelt "TE" and called every tight
    // end after the first a backup: twenty points off the second and the
    // third refused outright. The league's own allocation gives this
    // REC_FLEX to tight ends, so it starts two of them.
    let te_premium = rules(&["QB", "RB", "RB", "WR", "WR", "TE", "REC_FLEX", "BN"]);
    let board = te_premium_board();
    let demand = starting_demand(pool(&board), &te_premium, 12);
    assert!(
        demand["TE"] > 1.9,
        "the REC_FLEX went somewhere else: {demand:?}"
    );

    let candidates = [player("te2", "TE", 40.0), player("te3", "TE", 40.0)];
    let mut board = board;
    board.extend(candidates.iter().cloned());
    let mine = roster_via_rules(&te_premium, &["QB", "RB", "RB", "WR", "WR", "TE"]);
    let inputs = RecommendInputs::new(&board, Some(&mine), &te_premium, 8, 15, 90, 12);
    let ctx = context(
        &inputs,
        HashMap::from([("QB", 1), ("RB", 2), ("WR", 2), ("TE", 1)]),
    );
    let te2 = score_candidate(&ctx, &candidates[0], Mode::Balanced).expect("a TE");
    let said = te2.into_reasons();
    assert!(
        !said.iter().any(|r| r.contains("backup TE")),
        "the second tight end is a starter here: {said:?}"
    );

    // And the third is ordinary depth rather than a candidate refused
    // outright, which the name test did as soon as two were rostered.
    let ctx = context(
        &inputs,
        HashMap::from([("QB", 1), ("RB", 2), ("WR", 2), ("TE", 2)]),
    );
    assert!(
        score_candidate(&ctx, &candidates[1], Mode::Balanced).is_some(),
        "a third tight end in a two-TE league is depth, not a disqualification"
    );
}

// ---------- demand is a property of the league, not of what is left ----------

#[test]
fn superflex_demand_holds_up_after_the_quarterbacks_have_gone() {
    // Allocated against the remaining pool, a superflex slot stopped going to
    // quarterbacks the moment the pool ran shorter than the demand index, and
    // the roster that most needed a second quarterback was told the league
    // started one.
    let superflex = rules(&["QB", "RB", "RB", "WR", "WR", "TE", "SUPER_FLEX", "BN"]);
    let board = deep_board();
    let full: Vec<crate::board::BoardPlayer> = board.iter().map(|a| a.player.clone()).collect();
    // Twenty quarterbacks off the board, which is a normal round eight in a
    // twelve-team superflex league.
    let left: Vec<AvailablePlayer> = board
        .iter()
        .filter(|a| {
            !a.player.player_id.starts_with("QB")
                || a.player.player_id["QB".len()..]
                    .parse::<u32>()
                    .is_ok_and(|n| n >= 20)
        })
        .cloned()
        .collect();

    let mut inputs = RecommendInputs::new(&left, None, &superflex, 8, 15, 90, 12);
    let thin = starting_demand(inputs.demand_pool(), &superflex, 12);
    inputs.full_board = &full;
    let whole = starting_demand(inputs.demand_pool(), &superflex, 12);
    assert!(
        whole["QB"] > 1.9,
        "the league still starts two quarterbacks: {whole:?}"
    );
    assert!(
        thin["QB"] < whole["QB"],
        "this test is guarding nothing if the remaining pool gives the same answer"
    );
}

// ---------- one hole, one reason ----------

#[test]
fn one_open_starting_slot_is_paid_for_once() {
    // Three terms priced the same empty receiver room: the need bonus, the
    // thin-room warning, and the early-depth term. Two of them are gated on
    // the need bonus having fired; the third was not.
    let available = vec![player("wr", "WR", 10.0)];
    let standard = RosterRules::new(&slots());
    let mine = roster_with(&["QB", "RB", "RB", "TE"], &[("WR", 1)]);
    let inputs = RecommendInputs::new(&available, Some(&mine), &standard, 4, 15, 40, 12);
    let ctx = context(
        &inputs,
        HashMap::from([("QB", 1), ("RB", 2), ("WR", 0), ("TE", 1)]),
    );
    let reasons = score_candidate(&ctx, &available[0], Mode::Balanced)
        .expect("a receiver")
        .into_reasons();
    let need_reasons = reasons
        .iter()
        .filter(|r| r.contains("starter slot") || r.contains("thin at") || r.contains("empty slot"))
        .count();
    assert_eq!(
        need_reasons, 1,
        "one hole, {need_reasons} reasons: {reasons:?}"
    );
}
