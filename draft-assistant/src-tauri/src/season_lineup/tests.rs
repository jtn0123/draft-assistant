use super::*;

fn rules(slots: &[&str]) -> RosterRules {
    RosterRules::new(&slots.iter().map(|s| (*s).to_string()).collect::<Vec<_>>())
}

fn candidate(id: &str, position: &str, points: f64) -> Candidate {
    Candidate {
        player_id: id.into(),
        position: position.into(),
        points,
    }
}

fn ids(lineup: &[LineupSlot]) -> Vec<&str> {
    lineup
        .iter()
        .map(|s| s.player_id.as_deref().unwrap_or("-"))
        .collect()
}

#[test]
fn flex_takes_the_leftover_rather_than_stealing_a_dedicated_slot() {
    let rules = rules(&["RB", "WR", "FLEX", "BN"]);
    let lineup = optimal_lineup(
        &rules,
        &[
            candidate("rb1", "RB", 20.0),
            candidate("rb2", "RB", 18.0),
            candidate("wr1", "WR", 15.0),
        ],
    );
    // The RB slot must not be left empty just because FLEX came first.
    assert_eq!(ids(&lineup), vec!["rb1", "wr1", "rb2"]);
}

#[test]
fn superflex_is_filled_after_narrower_slots() {
    let rules = rules(&["SUPER_FLEX", "QB", "RB"]);
    let lineup = optimal_lineup(
        &rules,
        &[
            candidate("qb1", "QB", 25.0),
            candidate("qb2", "QB", 22.0),
            candidate("rb1", "RB", 20.0),
        ],
    );
    // Displayed in league order: SUPER_FLEX, QB, RB.
    assert_eq!(ids(&lineup), vec!["qb2", "qb1", "rb1"]);
}

#[test]
fn short_rosters_report_an_empty_slot_instead_of_dropping_it() {
    let rules = rules(&["QB", "TE"]);
    let lineup = optimal_lineup(&rules, &[candidate("qb1", "QB", 25.0)]);
    assert_eq!(ids(&lineup), vec!["qb1", "-"]);
    assert_eq!(lineup.len(), 2);
}

fn describe(id: &str) -> (String, Option<String>) {
    (id.to_uppercase(), Some("PIT".into()))
}

fn reason(_slot: &str, _a: &str, _b: &str) -> String {
    "because".into()
}

fn any(_slot: &str, _id: &str) -> bool {
    true
}

fn slot(slot: &str, id: Option<&str>, points: f64) -> LineupSlot {
    LineupSlot {
        slot: slot.into(),
        player_id: id.map(str::to_string),
        points,
    }
}

#[test]
fn a_shuffled_lineup_still_reports_the_one_starter_who_should_sit() {
    // Real week-1 case: WR and a FLEX are swapped relative to the optimal
    // lineup (harmless), but Pollard is set at FLEX over Downs on the
    // bench. Slot-by-slot pairing hid that call entirely.
    let optimal = vec![
        slot("WR", Some("watson"), 14.3),
        slot("FLEX", Some("wilson"), 14.0),
        slot("FLEX", Some("downs"), 12.8),
    ];
    let current = vec![
        slot("WR", Some("wilson"), 14.0),
        slot("FLEX", Some("pollard"), 9.1),
        slot("FLEX", Some("watson"), 14.3),
    ];
    let eligible = |slot: &str, id: &str| slot == "FLEX" || id != "downs";
    let calls = calls_from_diff(&optimal, &current, &eligible, &describe, &reason);
    assert_eq!(calls.len(), 1);
    assert_eq!(calls[0].player_in, "DOWNS");
    assert_eq!(calls[0].player_out, "POLLARD");
    assert_eq!(calls[0].slot, "FLEX");
    assert!((calls[0].gain - 3.7).abs() < 1e-9);
}

#[test]
fn incoming_players_prefer_a_slot_they_can_fill() {
    // A TE coming in must displace the weak TE, not the even weaker RB
    // sitting in a slot the TE cannot occupy.
    let optimal = vec![
        slot("RB", Some("rb"), 10.0),
        slot("TE", Some("te_good"), 11.0),
    ];
    let current = vec![
        slot("RB", Some("rb_weak"), 4.0),
        slot("TE", Some("te_weak"), 6.0),
    ];
    let eligible = |slot: &str, id: &str| {
        (slot == "TE" && id.starts_with("te")) || (slot == "RB" && id.starts_with("rb"))
    };
    let calls = calls_from_diff(&optimal, &current, &eligible, &describe, &reason);
    let te = calls
        .iter()
        .find(|c| c.player_in == "TE_GOOD")
        .expect("te call");
    assert_eq!(te.player_out, "TE_WEAK");
    assert_eq!(te.slot, "TE");
}

#[test]
fn moving_a_starter_between_eligible_slots_is_not_a_call() {
    let optimal = vec![
        LineupSlot {
            slot: "RB".into(),
            player_id: Some("a".into()),
            points: 10.0,
        },
        LineupSlot {
            slot: "FLEX".into(),
            player_id: Some("b".into()),
            points: 9.0,
        },
    ];
    let current = vec![
        LineupSlot {
            slot: "RB".into(),
            player_id: Some("b".into()),
            points: 9.0,
        },
        LineupSlot {
            slot: "FLEX".into(),
            player_id: Some("a".into()),
            points: 10.0,
        },
    ];
    assert!(calls_from_diff(&optimal, &current, &any, &describe, &reason).is_empty());
}

#[test]
fn benching_a_starter_for_a_better_one_is_a_call_sorted_by_gain() {
    let optimal = vec![
        LineupSlot {
            slot: "RB".into(),
            player_id: Some("good".into()),
            points: 18.0,
        },
        LineupSlot {
            slot: "WR".into(),
            player_id: Some("best".into()),
            points: 20.0,
        },
    ];
    let current = vec![
        LineupSlot {
            slot: "RB".into(),
            player_id: Some("bad".into()),
            points: 16.0,
        },
        LineupSlot {
            slot: "WR".into(),
            player_id: Some("worse".into()),
            points: 12.0,
        },
    ];
    let calls = calls_from_diff(&optimal, &current, &any, &describe, &reason);
    assert_eq!(calls.len(), 2);
    assert_eq!(calls[0].player_in, "BEST");
    assert!((calls[0].gain - 8.0).abs() < 1e-9);
    assert_eq!(calls[1].player_in, "GOOD");
}

#[test]
fn an_optimal_lineup_totals_a_positive_zero_gain() {
    // f64's additive identity is -0.0, which would serialise as "-0.0" and
    // read as a negative number of points left on the table.
    let lineup = vec![LineupSlot {
        slot: "QB".into(),
        player_id: Some("a".into()),
        points: 10.0,
    }];
    let calls = calls_from_diff(&lineup, &lineup, &any, &describe, &reason);
    assert!(calls.is_empty());
    let total: f64 = calls.iter().map(|c| c.gain).sum::<f64>() + 0.0;
    assert!(total.is_sign_positive(), "expected +0.0, got {total}");
}

#[test]
fn an_empty_starting_slot_is_reported_as_a_call() {
    let optimal = vec![LineupSlot {
        slot: "TE".into(),
        player_id: Some("te1".into()),
        points: 11.0,
    }];
    let current = vec![LineupSlot {
        slot: "TE".into(),
        player_id: None,
        points: 0.0,
    }];
    let calls = calls_from_diff(&optimal, &current, &any, &describe, &reason);
    assert_eq!(calls.len(), 1);
    assert_eq!(calls[0].player_out, "an empty slot");
}
