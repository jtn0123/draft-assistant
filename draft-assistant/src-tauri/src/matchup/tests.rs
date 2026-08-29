//! Tests for the lineup check and matchup preview (`matchup.rs`), in
//! their own file for the 500-line cap.

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

fn rules() -> RosterRules {
    RosterRules::new(
        &["QB", "RB", "WR", "FLEX", "DEF", "BN"]
            .iter()
            .map(|s| s.to_string())
            .collect::<Vec<_>>(),
    )
}

fn week() -> Vec<Candidate> {
    vec![
        c("qb", "QB", 20.0),
        c("rb1", "RB", 18.0),
        c("rb2", "RB", 9.0),
        c("wr1", "WR", 15.0),
        c("wr2", "WR", 11.0),
        c("def", "DEF", 7.0),
    ]
}

#[test]
fn a_worse_flex_and_an_empty_slot_are_both_reported() {
    // Set: rb2 in FLEX over wr2, and no DEF.
    let set: Vec<String> = ["qb", "rb1", "wr1", "rb2", "0"]
        .iter()
        .map(|s| s.to_string())
        .collect();
    let check = lineup_check(&set, &week(), &rules());
    assert_eq!(check.set_points, 62.0);
    assert_eq!(check.best_points, 71.0);
    assert_eq!(check.empty_slots, vec!["DEF"]);
    let changes: Vec<(&str, Option<&str>, &str)> = check
        .changes
        .iter()
        .map(|x| {
            (
                x.slot.as_str(),
                x.out.as_ref().map(|o| o.player_id.as_str()),
                x.in_.player_id.as_str(),
            )
        })
        .collect();
    assert_eq!(
        changes,
        vec![("FLEX", Some("rb2"), "wr2"), ("DEF", None, "def")]
    );
    assert!((check.changes[0].gain - 2.0).abs() < 1e-9);
}

#[test]
fn a_slot_nobody_can_fill_is_reported_empty_with_no_change_to_make() {
    // No DEF on the roster at all; the slot is set to "0".
    let roster: Vec<Candidate> = week().into_iter().filter(|c| c.position != "DEF").collect();
    let set: Vec<String> = ["qb", "rb1", "wr1", "wr2", "0"]
        .iter()
        .map(|s| s.to_string())
        .collect();
    let check = lineup_check(&set, &roster, &rules());
    assert_eq!(check.empty_slots, vec!["DEF"]);
    assert!(check.changes.is_empty(), "{:?}", check.changes);
}

#[test]
fn the_best_lineup_set_needs_no_changes() {
    let set: Vec<String> = ["qb", "rb1", "wr1", "wr2", "def"]
        .iter()
        .map(|s| s.to_string())
        .collect();
    let check = lineup_check(&set, &week(), &rules());
    assert!(check.changes.is_empty(), "{:?}", check.changes);
    assert!(check.empty_slots.is_empty());
    assert_eq!(check.set_points, check.best_points);
}

#[test]
fn the_same_players_in_swapped_slots_is_not_a_change() {
    // wr2 in WR, wr1 in FLEX: same nine points, different order.
    let set: Vec<String> = ["qb", "rb1", "wr2", "wr1", "def"]
        .iter()
        .map(|s| s.to_string())
        .collect();
    let check = lineup_check(&set, &week(), &rules());
    assert!(check.changes.is_empty(), "{:?}", check.changes);
}

#[test]
fn a_projected_favourite_is_more_likely_to_win_and_a_tie_is_a_coin_flip() {
    let theirs = vec![
        c("tqb", "QB", 18.0),
        c("trb", "RB", 12.0),
        c("twr", "WR", 12.0),
        c("twr2", "WR", 8.0),
        c("tdef", "DEF", 6.0),
    ];
    let p = preview(
        &week(),
        (5, Some("Them".into()), &[], &theirs),
        &rules(),
        &Teams::new(),
    );
    assert_eq!(p.opponent_points, 56.0);
    assert!(p.margin > 0.0);
    assert!(
        p.win_probability > 0.5 && p.win_probability < 1.0,
        "{}",
        p.win_probability
    );
    let even = preview(&week(), (5, None, &[], &week()), &rules(), &Teams::new());
    assert!(
        (even.win_probability - 0.5).abs() < 1e-6,
        "{}",
        even.win_probability
    );
}

#[test]
fn the_opponents_set_lineup_is_used_when_they_have_one() {
    // They benched their best back.
    let theirs = week();
    let set: Vec<String> = ["qb", "rb2", "wr1", "wr2", "def"]
        .iter()
        .map(|s| s.to_string())
        .collect();
    let p = preview(&week(), (5, None, &set, &theirs), &rules(), &Teams::new());
    assert_eq!(p.opponent_points, 62.0);
}

#[test]
fn opponents_share_a_matchup_id() {
    let m = |roster_id: u32, matchup_id: u32| Matchup {
        roster_id,
        matchup_id: Some(matchup_id),
        starters: Vec::new(),
        players: Vec::new(),
        points: 0.0,
        players_points: Default::default(),
    };
    let ms = [m(1, 7), m(3, 7), m(2, 4)];
    assert_eq!(opponent_roster_id(&ms, 1), Some(3));
    assert_eq!(opponent_roster_id(&ms, 2), None);
}

#[test]
fn a_stack_widens_the_spread_and_a_steadier_position_narrows_it() {
    let theirs = vec![
        c("tqb", "QB", 18.0),
        c("trb", "RB", 12.0),
        c("twr", "WR", 12.0),
        c("twr2", "WR", 8.0),
        c("tdef", "DEF", 6.0),
    ];
    let apart = preview(&week(), (5, None, &[], &theirs), &rules(), &Teams::new());
    // My QB and WR1 on the same NFL team: the same margin, less certain.
    let stacked: Teams = [("qb", "DET"), ("wr1", "DET")]
        .iter()
        .map(|(a, b)| (a.to_string(), b.to_string()))
        .collect();
    let together = preview(&week(), (5, None, &[], &theirs), &rules(), &stacked);
    assert_eq!(apart.margin, together.margin);
    assert!(
        together.win_probability < apart.win_probability,
        "{} vs {}",
        together.win_probability,
        apart.win_probability
    );
    assert!(position_cv("QB") < position_cv("DEF"));
}

#[test]
fn a_questionable_starter_is_flagged_but_still_counted() {
    let mut roster = week();
    roster[3].injury = Some("Questionable".into());
    let set: Vec<String> = ["qb", "rb1", "wr1", "wr2", "def"]
        .iter()
        .map(|s| s.to_string())
        .collect();
    let check = lineup_check(&set, &roster, &rules());
    assert!(check.changes.is_empty());
    assert_eq!(check.set_points, check.best_points);
    let flagged: Vec<&str> = check
        .questionable
        .iter()
        .map(|s| s.player_id.as_str())
        .collect();
    assert_eq!(flagged, vec!["wr1"]);
}

#[test]
fn an_out_starter_is_replaced_and_the_change_says_why() {
    let mut roster = week();
    // rb1 is set and Out; he scores nothing this week.
    roster[1].injury = Some("Out".into());
    roster[1].points = 0.0;
    let set: Vec<String> = ["qb", "rb1", "wr1", "wr2", "def"]
        .iter()
        .map(|s| s.to_string())
        .collect();
    let check = lineup_check(&set, &roster, &rules());
    assert_eq!(check.changes.len(), 1, "{:?}", check.changes);
    let ch = &check.changes[0];
    assert_eq!((ch.slot.as_str(), ch.in_.player_id.as_str()), ("RB", "rb2"));
    assert_eq!(
        ch.out.as_ref().and_then(|o| o.injury.as_deref()),
        Some("Out")
    );
    assert!((ch.gain - 9.0).abs() < 1e-9);
}
