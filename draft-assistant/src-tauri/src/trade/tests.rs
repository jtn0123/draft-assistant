//! Tests for the trade finder and evaluator (`trade.rs`), in their own
//! file for the 500-line cap.

use super::*;

fn c(id: &str, pos: &str, pts: f64) -> Candidate {
    Candidate {
        player_id: id.into(),
        name: id.into(),
        position: pos.into(),
        points: pts,
        bye_week: None,
        injury: None,
    }
}

#[test]
fn what_the_wire_gives_for_free_is_not_worth_trading_for() {
    let rules = RosterRules::new(
        &["RB", "WR", "DEF", "BN"]
            .iter()
            .map(|s| s.to_string())
            .collect::<Vec<_>>(),
    );
    // I have no defense; a free one scores 95, their spare scores 100.
    let mine = vec![
        c("rb", "RB", 200.0),
        c("wr1", "WR", 190.0),
        c("wr2", "WR", 150.0),
    ];
    let my_base = lineup::season_points(&mine, &rules);
    let free_def = crate::board::BoardPlayer {
        player_id: "def_free".into(),
        name: "Free DEF".into(),
        position: "DEF".into(),
        team: None,
        bye_week: None,
        points: 95.0,
        bonus_points: 0.0,
        vorp: 0.0,
        tier: 1,
        position_rank: 1,
        overall_rank: 1,
        adp: None,
        injury_status: None,
        sleeper_pts_ppr: None,
    };
    let free = free_gain_by_position(&mine, my_base, &[&free_def], &rules);
    assert!((free["DEF"] - 95.0).abs() < 1e-9);
    let their_def = c("def_theirs", "DEF", 100.0);
    let gain = lineup::season_points(&swapped(&mine, "wr2", &their_def), &rules) - my_base;
    let over_waiver = gain - free["DEF"];
    // Adds a hundred to my lineup — five over the free one. Not a trade.
    assert!(gain > 90.0 && over_waiver < 10.0, "{gain} / {over_waiver}");
}

#[test]
fn a_swap_that_helps_both_sides_is_found_from_the_lineup_math() {
    // Slots RB, WR, FLEX. I have three good WRs and one bad RB; they
    // have three good RBs and one bad WR. My WR3 for their RB3 lifts
    // both starting lineups.
    let rules = RosterRules::new(
        &["RB", "WR", "FLEX", "BN"]
            .iter()
            .map(|s| s.to_string())
            .collect::<Vec<_>>(),
    );
    let mine = vec![
        c("rb_bad", "RB", 60.0),
        c("wr1", "WR", 200.0),
        c("wr2", "WR", 180.0),
        c("wr3", "WR", 170.0),
    ];
    let theirs = vec![
        c("rb1", "RB", 210.0),
        c("rb2", "RB", 190.0),
        c("rb3", "RB", 175.0),
        c("wr_bad", "WR", 50.0),
    ];
    let my_base = lineup::season_points(&mine, &rules);
    let their_base = lineup::season_points(&theirs, &rules);
    let my_after = lineup::season_points(&swapped(&mine, "wr3", &theirs[2]), &rules);
    let their_after = lineup::season_points(&swapped(&theirs, "rb3", &mine[3]), &rules);
    // I start rb3 (175) in RB instead of rb_bad (60): +115. They start
    // wr3 (170) at WR instead of wr_bad (50): +120.
    assert!(
        (my_after - my_base - 115.0).abs() < 1e-9,
        "{}",
        my_after - my_base
    );
    assert!(
        (their_after - their_base - 120.0).abs() < 1e-9,
        "{}",
        their_after - their_base
    );
}

#[test]
fn an_offer_is_priced_both_ways_and_a_stranger_is_refused() {
    use crate::board::BoardPlayer;
    use crate::draft::RosterEntry;
    let rules = RosterRules::new(
        &["RB", "WR", "BN"]
            .iter()
            .map(|s| s.to_string())
            .collect::<Vec<_>>(),
    );
    let bp = |id: &str, pos: &str, pts: f64| BoardPlayer {
        player_id: id.into(),
        name: id.into(),
        position: pos.into(),
        team: None,
        bye_week: None,
        points: pts,
        bonus_points: 0.0,
        vorp: 0.0,
        tier: 1,
        position_rank: 1,
        overall_rank: 1,
        adp: None,
        injury_status: None,
        sleeper_pts_ppr: None,
    };
    let board = vec![
        bp("rb_a", "RB", 200.0),
        bp("wr_a", "WR", 100.0),
        bp("rb_b", "RB", 120.0),
        bp("wr_b", "WR", 190.0),
    ];
    let board_index: HashMap<String, usize> = board
        .iter()
        .enumerate()
        .map(|(i, p)| (p.player_id.clone(), i))
        .collect();
    let entry = |id: &str, pos: &str| RosterEntry {
        player_id: id.into(),
        name: id.into(),
        position: pos.into(),
        team: None,
        pick_no: 1,
        round: 1,
        is_keeper: false,
    };
    let roster = |slot: u32, ids: &[(&str, &str)]| TeamRoster {
        slot,
        display_name: Some(format!("T{slot}")),
        players: ids.iter().map(|(i, p)| entry(i, p)).collect(),
        open_starters: Vec::new(),
    };
    // I have the good RB and a weak WR; they have the reverse.
    let rosters = vec![
        roster(1, &[("rb_a", "RB"), ("wr_a", "WR")]),
        roster(2, &[("rb_b", "RB"), ("wr_b", "WR")]),
    ];
    let loaded = LoadedLeague {
        board,
        board_index,
        ..LoadedLeague::empty_for_tests()
    };
    let give = ["wr_a".to_string()];
    let get = ["wr_b".to_string()];
    let offer = Offer {
        my_slot: 1,
        partner_slot: 2,
        give: &give,
        get: &get,
        week: 1,
    };
    let v = evaluate(&loaded, &rosters, &offer, &rules).unwrap();
    let close = |a: f64, b: f64| (a - b).abs() < 1e-6;
    assert!(
        close(v.my_season_before, 300.0) && close(v.my_season_after, 390.0),
        "{v:?}"
    );
    assert!(
        close(v.their_season_before, 310.0) && close(v.their_season_after, 220.0),
        "{v:?}"
    );
    assert_eq!(v.partner_name.as_deref(), Some("T2"));
    let stranger = ["rb_b".to_string()];
    let bad = Offer {
        give: &stranger,
        get: &[],
        ..offer
    };
    let err = evaluate(&loaded, &rosters, &bad, &rules).unwrap_err();
    assert!(err.contains("not on my roster"), "{err}");
}

#[test]
fn a_second_piece_is_added_only_when_it_is_what_makes_them_say_yes() {
    use crate::draft::RosterEntry;
    let rules = RosterRules::new(
        &["RB", "WR", "WR", "BN", "BN"]
            .iter()
            .map(|s| s.to_string())
            .collect::<Vec<_>>(),
    );
    let bp = |id: &str, pos: &str, pts: f64| crate::board::BoardPlayer {
        player_id: id.into(),
        name: id.into(),
        position: pos.into(),
        team: None,
        bye_week: None,
        points: pts,
        bonus_points: 0.0,
        vorp: 0.0,
        tier: 1,
        position_rank: 1,
        overall_rank: 1,
        adp: None,
        injury_status: None,
        sleeper_pts_ppr: None,
    };
    // Their receivers both sit at 120. One of mine alone moves them by
    // two (122) or one and a half (121.5), under the bar; the pair lifts
    // both slots for +3.5. A pair only beats a single when both of theirs
    // are mediocre — which is exactly the roster a two-for-one is for. My fourth receiver
    // keeps my second WR slot filled after the two-for-one.
    let board = vec![
        bp("my_rb", "RB", 60.0),
        bp("my_wr1", "WR", 200.0),
        bp("my_wr2", "WR", 122.0),
        bp("my_wr3", "WR", 121.5),
        bp("my_wr4", "WR", 100.0),
        bp("their_rb1", "RB", 220.0),
        bp("their_rb2", "RB", 180.0),
        bp("their_wr1", "WR", 120.0),
        bp("their_wr2", "WR", 120.0),
    ];
    let board_index: HashMap<String, usize> = board
        .iter()
        .enumerate()
        .map(|(i, p)| (p.player_id.clone(), i))
        .collect();
    let entry = |id: &str, pos: &str| RosterEntry {
        player_id: id.into(),
        name: id.into(),
        position: pos.into(),
        team: None,
        pick_no: 1,
        round: 1,
        is_keeper: false,
    };
    let rosters = vec![
        TeamRoster {
            slot: 1,
            display_name: Some("Me".into()),
            players: vec![
                entry("my_rb", "RB"),
                entry("my_wr1", "WR"),
                entry("my_wr2", "WR"),
                entry("my_wr3", "WR"),
                entry("my_wr4", "WR"),
            ],
            open_starters: Vec::new(),
        },
        TeamRoster {
            slot: 2,
            display_name: Some("Them".into()),
            players: vec![
                entry("their_rb1", "RB"),
                entry("their_rb2", "RB"),
                entry("their_wr1", "WR"),
                entry("their_wr2", "WR"),
            ],
            open_starters: Vec::new(),
        },
    ];
    let loaded = LoadedLeague {
        board,
        board_index,
        ..LoadedLeague::empty_for_tests()
    };
    let ideas = ideas(&loaded, &rosters, 1, &[], &rules);
    let two = ideas
        .iter()
        .find(|i| i.get_id == "their_rb2")
        .expect("their spare back is the target: {ideas:?}");
    assert!(
        two.also_give_id.is_some(),
        "one receiver does not move them; two do: {two:?}"
    );
    assert!(two.their_gain >= MIN_GAIN && two.over_waiver >= MIN_GAIN);
}
