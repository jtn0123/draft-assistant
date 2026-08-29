//! The recommendation engine's rules, exercised through its public entry
//! point. Lifted out of `recommend.rs` when that file crossed the 500-line
//! cap; nothing here reaches for a private item.

use draft_assistant_lib::board::{AvailablePlayer, BoardPlayer};
use draft_assistant_lib::draft::{RosterEntry, TeamRoster};
use draft_assistant_lib::recommend::{recommend, serious_injury};
use draft_assistant_lib::roster::RosterRules;

fn player(id: &str, pos: &str, vorp: f64) -> AvailablePlayer {
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
        },
        survival_next: None,
    }
}

fn entry(pos: &str, n: u32) -> RosterEntry {
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

fn roster(positions: &[&str]) -> TeamRoster {
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

fn slots() -> Vec<String> {
    [
        "QB", "RB", "WR", "TE", "FLEX", "FLEX", "FLEX", "FLEX", "DEF", "BN",
    ]
    .iter()
    .map(|s| s.to_string())
    .collect()
}

#[test]
fn never_recommends_second_def() {
    // A monster-VORP second DEF must lose to a modest RB.
    let available = vec![player("def2", "DEF", 90.0), player("rb1", "RB", 30.0)];
    let mine = roster(&["QB", "RB", "WR", "TE", "DEF"]);
    let recs = recommend(
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
    let recs = recommend(
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
    let recs = recommend(
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
fn fallback_when_all_disqualified() {
    // Only a second DEF available — fallback must still recommend it.
    let available = vec![player("def2", "DEF", 50.0)];
    let mine = roster(&["QB", "RB", "WR", "TE", "DEF"]);
    let recs = recommend(
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

    let recs = recommend(
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

/// Draft night, pick 30: four receivers sat in one band while the running
/// backs were about to lose four bodies in seven picks. The old score was
/// 0.6 per VORP point against a flat six for "he will not last", so the
/// deeper position always won and the run was described afterwards rather
/// than anticipated.
#[test]
fn the_scarce_position_wins_when_waiting_actually_costs_you() {
    let with_odds = |id: &str, pos: &str, vorp: f64, surv: f64| AvailablePlayer {
        survival_next: Some(surv),
        ..player(id, pos, vorp)
    };
    let mut available = vec![
        // A receiver band four deep: losing the best one costs little.
        with_odds("wr1", "WR", 70.0, 0.8),
        with_odds("wr2", "WR", 68.0, 0.8),
        with_odds("wr3", "WR", 66.0, 0.8),
        with_odds("wr4", "WR", 64.0, 0.8),
        // One back worth having, and a cliff behind him.
        with_odds("rb1", "RB", 60.0, 0.25),
        with_odds("rb2", "RB", 15.0, 0.9),
    ];
    available.sort_by(|a, b| b.player.vorp.total_cmp(&a.player.vorp));

    let mine = roster(&["QB", "TE"]);
    let slots = ["QB", "RB", "WR", "TE", "FLEX", "BN"]
        .iter()
        .map(|s| (*s).to_string())
        .collect::<Vec<_>>();
    let recs = recommend(
        &available,
        Some(&mine),
        &RosterRules::new(&slots),
        3,
        15,
        30,
    );

    assert_eq!(recs[0].player_id, "rb1", "{recs:?}");
    assert!(
        recs[0]
            .reasons
            .iter()
            .any(|r| r.contains("better than what RB is likely to offer")),
        "{:?}",
        recs[0].reasons
    );
}

#[test]
fn a_deep_position_says_so_rather_than_urging_you_on() {
    let available: Vec<AvailablePlayer> = (0..5)
        .map(|i| AvailablePlayer {
            survival_next: Some(0.85),
            ..player(&format!("wr{i}"), "WR", 70.0 - f64::from(i))
        })
        .collect();
    let mine = roster(&["QB", "TE"]);
    let slots = ["QB", "RB", "WR", "TE", "FLEX", "BN"]
        .iter()
        .map(|s| (*s).to_string())
        .collect::<Vec<_>>();
    let recs = recommend(
        &available,
        Some(&mine),
        &RosterRules::new(&slots),
        3,
        15,
        30,
    );
    assert!(
        recs[0].reasons.iter().any(|r| r.contains("WR is deep")),
        "{:?}",
        recs[0].reasons
    );
}

#[test]
fn never_recommends_a_second_kicker() {
    let available = vec![player("k2", "K", 100.0), player("rb2", "RB", 10.0)];
    let mine = roster(&["QB", "RB", "WR", "TE", "K"]);
    let kicker_slots = ["QB", "RB", "WR", "TE", "K", "BN"]
        .iter()
        .map(|slot| (*slot).to_string())
        .collect::<Vec<_>>();
    let recs = recommend(
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

fn flagged(id: &str, vorp: f64, status: &str) -> AvailablePlayer {
    let mut p = player(id, "WR", vorp);
    p.player.injury_status = Some(status.into());
    p
}

#[test]
fn safe_mode_ignores_a_preseason_questionable_tag_but_not_out() {
    let rules = RosterRules::new(&slots());
    let mine = roster(&["QB", "RB"]);
    let questionable = vec![flagged("q", 30.0, "Questionable"), player("h", "WR", 25.0)];
    let recs = recommend(&questionable, Some(&mine), &rules, 14, 15, 30);
    let safe = recs.iter().find(|r| r.mode == "safe").unwrap();
    assert_eq!(safe.player_id, "q", "{recs:?}");
    assert!(
        safe.reasons.iter().all(|r| !r.starts_with("injury flag")),
        "{recs:?}"
    );

    let out = vec![flagged("o", 30.0, "Out"), player("h", "WR", 25.0)];
    let recs = recommend(&out, Some(&mine), &rules, 14, 15, 30);
    let safe = recs.iter().find(|r| r.mode == "safe").unwrap();
    assert_eq!(safe.player_id, "h", "{recs:?}");
}

#[test]
fn serious_statuses_are_case_insensitive() {
    assert!(serious_injury("IR"));
    assert!(serious_injury("out"));
    assert!(serious_injury("Doubtful"));
    assert!(!serious_injury("Questionable"));
    assert!(!serious_injury(""));
}

// ---------- what the app got wrong on draft night ----------

/// Named for the pick it produced: with the running backs picked over, every
/// one left was below replacement, and the drop-off between the best of them
/// and the expected best of them came out larger than any real gap on the
/// board.
#[test]
fn a_barren_position_does_not_manufacture_a_cliff() {
    let available = vec![
        player("rb_scraps1", "RB", -2.0),
        player("rb_scraps2", "RB", -6.0),
        player("wr_real", "WR", 18.0),
    ];
    let mut mine = roster(&["QB", "RB", "RB", "WR", "WR", "TE"]);
    mine.open_starters = vec![("DEF".into(), 1)];
    let recs = recommend(
        &available,
        Some(&mine),
        &RosterRules::new(&slots()),
        7,
        15,
        94,
    );
    // A minus-two back is worth no more than the waiver wire. Whatever the
    // engine picks, it must not be him over an eighteen-VORP receiver.
    assert!(
        recs.iter().all(|r| r.player_id != "rb_scraps1"),
        "recommended a below-replacement back over real value: {recs:?}"
    );
}

/// Four flex slots said yes to every skill position equally, so once RB, WR
/// and TE were each covered once the receivers won every tiebreak on raw
/// value — five of them, against two backs.
#[test]
fn the_sixth_receiver_loses_to_the_third_back_at_similar_value() {
    let available = vec![player("wr6", "WR", 12.0), player("rb3", "RB", 8.0)];
    let mut mine = roster(&["QB", "RB", "RB", "WR", "WR", "WR", "WR", "WR", "TE"]);
    mine.open_starters = vec![("DEF".into(), 1)];
    let recs = recommend(
        &available,
        Some(&mine),
        &RosterRules::new(&slots()),
        8,
        15,
        105,
    );
    assert!(
        recs.iter().all(|r| r.position == "RB"),
        "took a sixth receiver behind five of his own: {recs:?}"
    );
}

/// The gap still has to be crossable: this is a tilt, not a ban. A receiver
/// far enough ahead is still the pick.
#[test]
fn a_receiver_far_enough_ahead_still_beats_the_third_back() {
    let available = vec![player("wr6", "WR", 60.0), player("rb3", "RB", 8.0)];
    let mut mine = roster(&["QB", "RB", "RB", "WR", "WR", "WR", "WR", "WR", "TE"]);
    mine.open_starters = vec![("DEF".into(), 1)];
    let recs = recommend(
        &available,
        Some(&mine),
        &RosterRules::new(&slots()),
        8,
        15,
        105,
    );
    assert!(
        recs.iter().all(|r| r.position == "WR"),
        "a 52-VORP gap should still win: {recs:?}"
    );
}

#[test]
fn two_receivers_from_one_offence_is_a_penalty() {
    let mut same = player("wr_same", "WR", 20.0);
    same.player.team = Some("CIN".into());
    let mut other = player("wr_other", "WR", 20.0);
    other.player.team = Some("SEA".into());

    let mut mine = roster(&["QB", "RB", "WR", "TE"]);
    mine.players[2].team = Some("CIN".into());
    mine.open_starters = vec![("FLEX".into(), 2)];

    let recs = recommend(
        &[same, other],
        Some(&mine),
        &RosterRules::new(&slots()),
        6,
        15,
        70,
    );
    assert!(
        recs.iter().all(|r| r.player_id == "wr_other"),
        "stacked a second receiver on the same offence at equal value: {recs:?}"
    );
}

#[test]
fn and_says_which_offence_it_is_avoiding() {
    let mut same = player("wr_same", "WR", 20.0);
    same.player.team = Some("CIN".into());
    let mut mine = roster(&["QB", "RB", "WR", "TE"]);
    mine.players[2].team = Some("CIN".into());
    mine.open_starters = vec![("FLEX".into(), 2)];
    // The only candidate, so he is recommended anyway — with the warning on.
    let recs = recommend(&[same], Some(&mine), &RosterRules::new(&slots()), 6, 15, 70);
    assert!(
        recs[0].reasons.iter().any(|r| r.contains("CIN")),
        "recommended a second Bengal without saying so: {:?}",
        recs[0].reasons
    );
}

#[test]
fn a_defense_waits_for_the_last_two_rounds() {
    let available = vec![player("def1", "DEF", 40.0), player("wr9", "WR", 42.0)];
    let mut mine = roster(&["QB", "RB", "RB", "WR", "WR", "WR", "TE"]);
    mine.open_starters = vec![("DEF".into(), 1), ("FLEX".into(), 1)];
    // Round 13 of 15 is still a round to spend on a flier.
    let recs = recommend(
        &available,
        Some(&mine),
        &RosterRules::new(&slots()),
        13,
        15,
        170,
    );
    assert!(
        recs.iter().all(|r| r.position != "DEF"),
        "took a defense with three rounds left: {recs:?}"
    );
}

/// Caught live at pick 121: with the backs picked over, "thin at RB" and
/// "last of his tier" were flat bonuses that applied to a back 24 points
/// *below* replacement, and together they carried him to the top of the board.
/// A position being thin is a reason to take a good player there, never a
/// reason to take a bad one.
#[test]
fn thinness_never_recommends_a_below_replacement_body() {
    let available = vec![
        player("rb_scraps", "RB", -24.0),
        player("wr_ok", "WR", 13.0),
    ];
    // Seven receivers, as on the night: deep enough that the old flat
    // "already N rostered" penalty stacked on top of the crowding discount
    // and buried the only player on the board worth having.
    let mut mine = roster(&[
        "QB", "RB", "RB", "WR", "WR", "WR", "WR", "WR", "WR", "WR", "TE",
    ]);
    mine.open_starters = vec![("DEF".into(), 1)];
    let recs = recommend(
        &available,
        Some(&mine),
        &RosterRules::new(&slots()),
        9,
        15,
        121,
    );
    assert!(
        recs.iter().all(|r| r.position == "WR"),
        "took a back worth less than the waiver wire: {recs:?}"
    );
}
