//! Roster discipline: who the recommender refuses to put forward, and the
//! fallback that keeps it from putting forward nobody.
//!
//! The helpers live here; what the three modes are *for* is next door in
//! `recommend_mode_tests.rs`.

use super::*;
use crate::board::AvailablePlayer;
use crate::board::BoardPlayer;
use crate::draft::RosterEntry;

pub(super) fn player(id: &str, pos: &str, vorp: f64) -> AvailablePlayer {
    AvailablePlayer {
        player: BoardPlayer {
            player_id: id.into(),
            name: id.into(),
            position: pos.into(),
            team: None,
            bye_week: None,
            points: 150.0 + vorp,
            bonus_points: 0.0,
            vorp,
            tier: 1,
            position_rank: 1,
            overall_rank: 1,
            adp: Some(100.0),
            injury_status: None,
            sleeper_pts_ppr: None,
            second_opinion: None,
            weekly_cv: None,
        },
        survival_next: None,
    }
}

/// A board deep enough for a twelve-team league to allocate its flex slots
/// against: forty bodies a position, at the levels those positions really
/// score at. The demand model reads the whole board, so a test that asks it
/// what a league starts has to give it one.
pub(super) fn deep_full_board() -> Vec<BoardPlayer> {
    let mut board = Vec::new();
    for (position, top) in [("QB", 380.0), ("RB", 300.0), ("WR", 295.0), ("TE", 220.0)] {
        for i in 0..40 {
            let mut p = player(&format!("deep_{position}{i}"), position, 0.0).player;
            p.points = top - 4.0 * f64::from(i);
            board.push(p);
        }
    }
    board
}

pub(super) fn entry(pos: &str, n: u32) -> RosterEntry {
    RosterEntry {
        player_id: format!("{pos}{n}"),
        name: format!("{pos}{n}"),
        position: pos.into(),
        team: None,
        pick_no: n,
        round: n,
        is_keeper: false,
    }
}

pub(super) fn roster(positions: &[&str]) -> TeamRoster {
    TeamRoster {
        slot: 2,
        display_name: None,
        players: positions
            .iter()
            .enumerate()
            .map(|(i, p)| entry(p, i as u32 + 1))
            .collect(),
        open_starters: vec![("FLEX".into(), 2)],
    }
}

pub(super) fn slots() -> Vec<String> {
    [
        "QB", "RB", "WR", "TE", "FLEX", "FLEX", "FLEX", "FLEX", "DEF", "BN",
    ]
    .iter()
    .map(|s| s.to_string())
    .collect()
}

/// The plain call: a board, a roster, a clock.
pub(super) fn recs(
    available: &[AvailablePlayer],
    mine: Option<&TeamRoster>,
    rules: &RosterRules,
    current_round: u32,
    total_rounds: u32,
    current_pick: u32,
) -> Vec<Recommendation> {
    recommend(&RecommendInputs::new(
        available,
        mine,
        rules,
        current_round,
        total_rounds,
        current_pick,
        12,
    ))
}

/// The same call with the whole board in hand. What a league starts is
/// worked out from every player the board knows, so any test that turns on
/// that number has to hand one over rather than leaning on the two players it
/// happens to be comparing.
pub(super) fn recs_on_board(
    available: &[AvailablePlayer],
    full_board: &[BoardPlayer],
    mine: Option<&TeamRoster>,
    rules: &RosterRules,
    current_round: u32,
    total_rounds: u32,
    current_pick: u32,
) -> Vec<Recommendation> {
    let mut inputs = RecommendInputs::new(
        available,
        mine,
        rules,
        current_round,
        total_rounds,
        current_pick,
        12,
    );
    inputs.full_board = full_board;
    recommend(&inputs)
}

pub(super) fn of_mode<'a>(recs: &'a [Recommendation], mode: &str) -> &'a Recommendation {
    recs.iter()
        .find(|r| r.mode == mode)
        .unwrap_or_else(|| panic!("no {mode} recommendation in {recs:?}"))
}

// ---------- roster discipline ----------

#[test]
fn never_recommends_second_def() {
    // A monster-VORP second DEF must lose to a modest RB.
    let available = vec![player("def2", "DEF", 90.0), player("rb1", "RB", 30.0)];
    let mine = roster(&["QB", "RB", "WR", "TE", "DEF"]);
    let recs = recs(
        &available,
        Some(&mine),
        &RosterRules::new(&slots()),
        14,
        15,
        180,
    );
    assert!(recs.iter().all(|r| r.position != "DEF"), "{recs:?}");
}

#[test]
fn never_recommends_third_qb() {
    let available = vec![player("qb3", "QB", 95.0), player("wr1", "WR", 20.0)];
    let mine = roster(&["QB", "QB", "RB", "WR"]);
    let recs = recs(
        &available,
        Some(&mine),
        &RosterRules::new(&slots()),
        10,
        15,
        130,
    );
    assert!(recs.iter().all(|r| r.position != "QB"), "{recs:?}");
}

#[test]
fn locks_def_in_final_rounds_when_missing() {
    let available = vec![player("def1", "DEF", 40.0), player("wr9", "WR", 42.0)];
    let mut mine = roster(&["QB", "RB", "RB", "WR", "WR", "WR", "TE"]);
    // As the engine would report it: the DEF starter slot is still open.
    mine.open_starters = vec![("DEF".into(), 1), ("FLEX".into(), 1)];
    let recs = recs(
        &available,
        Some(&mine),
        &RosterRules::new(&slots()),
        14,
        15,
        184,
    );
    assert_eq!(recs[0].position, "DEF", "{recs:?}");
}

#[test]
fn the_def_gate_is_rounds_remaining_not_a_round_number() {
    // The same round number means opposite things in a fifteen-round league
    // and in a three-round mock: what matters is how many picks are left to
    // spend on the one defence the roster ever needs.
    let available = vec![player("def1", "DEF", 40.0), player("wr9", "WR", 30.0)];
    let mut mine = roster(&["QB", "RB", "WR", "TE"]);
    mine.open_starters = vec![("DEF".into(), 1), ("FLEX".into(), 1)];
    let rules = RosterRules::new(&slots());
    // Round 3 of fifteen: twelve rounds to go, a defence is a waste.
    let long = recs(&available, Some(&mine), &rules, 3, 15, 30);
    assert!(long.iter().all(|r| r.position != "DEF"), "{long:?}");
    // Round 3 of three: this is the last chance to take one.
    let short = recs(&available, Some(&mine), &rules, 3, 3, 30);
    assert_eq!(of_mode(&short, "balanced").position, "DEF", "{short:?}");
}

#[test]
fn fallback_when_all_disqualified() {
    // Only a second DEF available — fallback must still recommend it.
    let available = vec![player("def2", "DEF", 50.0)];
    let mine = roster(&["QB", "RB", "WR", "TE", "DEF"]);
    let recs = recs(
        &available,
        Some(&mine),
        &RosterRules::new(&slots()),
        15,
        15,
        200,
    );
    assert!(!recs.is_empty());
}

#[test]
fn superflex_qb_is_recognized_as_filling_a_starter() {
    let available = vec![player("qb2", "QB", 80.0), player("wr2", "WR", 10.0)];
    let mut mine = roster(&["QB", "RB", "WR", "TE"]);
    mine.open_starters = vec![("SUPER_FLEX".into(), 1)];
    let superflex_slots = ["QB", "RB", "WR", "TE", "SUPER_FLEX", "BN"]
        .iter()
        .map(|slot| (*slot).to_string())
        .collect::<Vec<_>>();

    let recs = recs(
        &available,
        Some(&mine),
        &RosterRules::new(&superflex_slots),
        5,
        10,
        50,
    );

    assert_eq!(recs[0].player_id, "qb2", "{recs:?}");
    assert!(recs[0]
        .reasons
        .iter()
        .any(|reason| reason.contains("SUPER_FLEX")));
}

#[test]
fn never_recommends_a_second_kicker() {
    let available = vec![player("k2", "K", 100.0), player("rb2", "RB", 10.0)];
    let mine = roster(&["QB", "RB", "WR", "TE", "K"]);
    let kicker_slots = ["QB", "RB", "WR", "TE", "K", "BN"]
        .iter()
        .map(|slot| (*slot).to_string())
        .collect::<Vec<_>>();
    let recs = recs(
        &available,
        Some(&mine),
        &RosterRules::new(&kicker_slots),
        9,
        10,
        90,
    );
    assert!(recs
        .iter()
        .all(|recommendation| recommendation.position != "K"));
}
