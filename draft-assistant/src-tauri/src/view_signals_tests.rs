//! The derived draft signals, each read on its own: a validated seat, the
//! reason there is no seat, the pick clock, the survival window, bye clashes
//! and the positional run.

use super::*;

#[test]
fn invalid_user_slots_are_rejected_before_roster_indexing() {
    assert_eq!(validated_slot(Some(0), 14).0, None);
    assert_eq!(validated_slot(Some(15), 14).0, None);
    assert_eq!(validated_slot(Some(2), 14).0, Some(2));
}

#[test]
fn every_reason_for_having_no_seat_is_said_in_its_own_words() {
    // These five used to share one sentence, "set your Sleeper username",
    // shown to people who had set it hours ago. Each branch is pinned
    // because the whole point of the function is that they differ.
    let mut order = HashMap::new();
    order.insert("u1".to_string(), 1u32);
    let empty: HashMap<String, u32> = HashMap::new();

    // An out-of-range slot answers first, whatever else is true.
    assert_eq!(
        seat_note(crate::view_types::YAHOO, Some("u1"), Some(&order), true),
        "Your saved draft slot is not one this league has."
    );
    assert_eq!(
        seat_note(crate::view_types::YAHOO, Some("u1"), Some(&order), false),
        "Connect Yahoo to track your team."
    );
    assert_eq!(
        seat_note("sleeper", None, Some(&order), false),
        "Set your Sleeper username to track your team."
    );
    // A blank id is as unset as a missing one.
    assert_eq!(
        seat_note("sleeper", Some(""), Some(&order), false),
        "Set your Sleeper username to track your team."
    );
    // No order, and an order Sleeper returned empty, are the same thing:
    // the draft has not opened, and there is nothing to fix.
    assert_eq!(
        seat_note("sleeper", Some("u1"), None, false),
        "The draft order has not been posted yet."
    );
    assert_eq!(
        seat_note("sleeper", Some("u1"), Some(&empty), false),
        "The draft order has not been posted yet."
    );
    assert_eq!(
        seat_note("sleeper", Some("u9"), Some(&order), false),
        "You are not in this league."
    );
    // The order names the user, so a seat should have been found; kept
    // total rather than panicking.
    assert_eq!(
        seat_note("sleeper", Some("u1"), Some(&order), false),
        "Your seat in this draft could not be worked out."
    );
}

#[test]
fn clock_deadline_is_last_pick_plus_timer_only_while_drafting() {
    assert_eq!(
        clock_deadline_ms("drafting", Some(1_000), Some(90), None),
        Some(91_000)
    );
    assert_eq!(
        clock_deadline_ms("pre_draft", Some(1_000), Some(90), None),
        None
    );
    assert_eq!(
        clock_deadline_ms("complete", Some(1_000), Some(90), None),
        None
    );
    assert_eq!(clock_deadline_ms("drafting", None, Some(90), None), None);
    assert_eq!(clock_deadline_ms("drafting", Some(1_000), None, None), None);
    assert_eq!(
        clock_deadline_ms("drafting", Some(1_000), Some(0), None),
        None
    );
}

/// The first clock of every draft was blank until somebody picked, and a
/// keeper league opened on 0:00 because `last_picked` was the evening the
/// keepers were entered.
#[test]
fn the_opening_clock_runs_from_the_drafts_start_time() {
    // Pick one: nobody has picked, so the timer runs from the start.
    assert_eq!(
        clock_deadline_ms("drafting", None, Some(90), Some(1_000_000)),
        Some(1_090_000)
    );
    // A keeper league's last pick predates the start by a week. The clock
    // starts when the draft does, not when the keepers were entered.
    assert_eq!(
        clock_deadline_ms("drafting", Some(400_000), Some(90), Some(1_000_000)),
        Some(1_090_000)
    );
    // Once the draft is under way the last pick is the later stamp again.
    assert_eq!(
        clock_deadline_ms("drafting", Some(1_200_000), Some(90), Some(1_000_000)),
        Some(1_290_000)
    );
    // A start time is no help before the draft starts, or without a timer.
    assert_eq!(
        clock_deadline_ms("pre_draft", None, Some(90), Some(1_000_000)),
        None
    );
    assert_eq!(
        clock_deadline_ms("drafting", None, None, Some(1_000_000)),
        None
    );
    // Sleeper serves 0 for a draft with no scheduled start.
    assert_eq!(clock_deadline_ms("drafting", None, Some(90), Some(0)), None);
}

fn booked(picks: &[u32]) -> HashSet<u32> {
    picks.iter().copied().collect()
}

#[test]
fn a_snake_turn_is_priced_as_one_window() {
    let none = booked(&[]);
    // Slot 12 of twelve: picks 12 and 13 are back to back, then 36. What
    // I pass on now I do not see again until 36, not 13.
    assert_eq!(
        survival_target(&[12, 13, 36, 37], 12, true, &none),
        Some(36)
    );
    // Not on the clock, with my turn about to come round: the pair is
    // still one window and 36 is still the pick that matters.
    assert_eq!(survival_target(&[13, 36, 37], 12, false, &none), Some(36));
    // An ordinary pick in the middle of a round: my next pick is my next
    // pick.
    assert_eq!(survival_target(&[30, 43, 54], 30, true, &none), Some(43));
    assert_eq!(survival_target(&[43, 54], 30, false, &none), Some(43));
    // The last pick of the draft has nothing after it.
    assert_eq!(survival_target(&[180], 180, true, &none), None);
    assert_eq!(survival_target(&[], 180, false, &none), None);
    // A pair with nothing beyond it: the second half is all there is.
    assert_eq!(survival_target(&[12, 13], 12, true, &none), Some(13));
}

/// Three picks in a row, or a keeper sitting between two of mine, both
/// collapsed to "my own following pick" — and every player then read as
/// 99% to survive, at the moment the board is emptiest.
#[test]
fn a_window_runs_through_every_pick_that_will_not_actually_be_made() {
    let none = booked(&[]);
    // A traded pick leaves me 11, 12 and 13 in a row. The turn after that
    // run is 36, not 12 and not 13.
    assert_eq!(
        survival_target(&[11, 12, 13, 36], 11, true, &none),
        Some(36)
    );
    assert_eq!(
        survival_target(&[11, 12, 13, 36], 10, false, &none),
        Some(36)
    );
    // Picks 12 and 13 are keepers, already in the book: nobody selects
    // there, so 11 and 14 are adjacent and 35 is the pick that counts.
    let keepers = booked(&[12, 13]);
    assert_eq!(survival_target(&[11, 14, 35], 11, true, &keepers), Some(35));
    assert_eq!(
        survival_target(&[11, 14, 35], 10, false, &keepers),
        Some(35)
    );
    // A traded pick giving me 1.05 and 1.06 of a twelve-team draft: back
    // to back, so the next turn that matters is 20.
    assert_eq!(survival_target(&[5, 6, 20, 29], 5, true, &none), Some(20));
    // A keeper somewhere else entirely changes nothing.
    assert_eq!(
        survival_target(&[30, 43, 54], 30, true, &booked(&[177])),
        Some(43)
    );
}

fn standard_slots() -> RosterRules {
    RosterRules::new(
        &[
            "QB", "RB", "RB", "WR", "WR", "TE", "FLEX", "DEF", "BN", "BN",
        ]
        .iter()
        .map(|slot| (*slot).to_string())
        .collect::<Vec<_>>(),
    )
}

#[test]
fn only_the_players_who_would_start_carry_a_bye_clash() {
    let rules = standard_slots();
    // A full starting nine, all off in week 9, plus two bench receivers
    // who are also off in week 9. The lineup loses seven men, not nine.
    let mut roster: Vec<(&str, Option<u32>)> = vec![
        ("QB", Some(9)),
        ("RB", Some(9)),
        ("RB", Some(9)),
        ("WR", Some(9)),
        ("WR", Some(9)),
        ("TE", Some(9)),
        ("RB", Some(9)),
        ("DEF", Some(9)),
    ];
    roster.push(("WR", Some(9)));
    roster.push(("WR", Some(9)));
    let byes = starter_byes(&rules, roster);
    assert_eq!(byes.get(&9), Some(&8), "{byes:?}");
}

#[test]
fn a_bench_only_bye_is_not_a_lineup_problem() {
    let rules = standard_slots();
    // One starter at every slot on a clean week, and three spare backs
    // all off in week 7. Nothing in the lineup is missing that week.
    let roster: Vec<(&str, Option<u32>)> = vec![
        ("QB", Some(5)),
        ("RB", Some(5)),
        ("RB", Some(5)),
        ("WR", Some(5)),
        ("WR", Some(5)),
        ("TE", Some(5)),
        ("DEF", Some(5)),
        ("WR", Some(5)),
        ("RB", Some(7)),
        ("RB", Some(7)),
        ("WR", Some(7)),
    ];
    let byes = starter_byes(&rules, roster);
    assert_eq!(byes.get(&7), None, "{byes:?}");
    assert_eq!(byes.get(&5), Some(&8));
}

#[test]
fn a_flex_does_not_eat_the_only_body_a_dedicated_slot_needs() {
    let rules = standard_slots();
    // The single tight end has to start at TE, not be swallowed by the
    // FLEX that is listed before... after it — either way the dedicated
    // slots are filled first, so the FLEX takes the spare back.
    let roster: Vec<(&str, Option<u32>)> = vec![
        ("TE", Some(11)),
        ("RB", Some(3)),
        ("RB", Some(3)),
        ("RB", Some(11)),
    ];
    let byes = starter_byes(&rules, roster);
    assert_eq!(byes.get(&11), Some(&2), "{byes:?}");
    assert_eq!(byes.get(&3), Some(&2));
}

#[test]
fn an_idp_starters_bye_is_not_a_clash_for_the_lineup_the_board_builds() {
    // An IDP league's DL slot took the drafted lineman and counted his bye
    // among "your starters", though the board never drafts for that slot
    // and the open-starter count already leaves it out.
    let rules = RosterRules::new(
        &["QB", "RB", "DL", "LB", "BN"]
            .iter()
            .map(|slot| (*slot).to_string())
            .collect::<Vec<_>>(),
    );
    let roster: Vec<(&str, Option<u32>)> = vec![
        ("QB", Some(9)),
        ("DL", Some(9)),
        ("LB", Some(9)),
        ("RB", Some(4)),
    ];
    let byes = starter_byes(&rules, roster);
    assert_eq!(byes.get(&9), Some(&1), "{byes:?}");
    assert_eq!(byes.get(&4), Some(&1));
}

#[test]
fn a_player_with_no_known_bye_counts_against_no_week() {
    let rules = standard_slots();
    let roster: Vec<(&str, Option<u32>)> = vec![("QB", None), ("RB", Some(6))];
    let byes = starter_byes(&rules, roster);
    assert_eq!(byes.len(), 1);
    assert_eq!(byes.get(&6), Some(&1));
}

#[test]
fn position_run_carries_the_count_and_window() {
    let picks: Vec<String> = ["WR", "RB", "RB", "QB", "RB", "RB", "TE"]
        .iter()
        .map(|s| s.to_string())
        .collect();
    // Last six: RB RB QB RB RB TE -> four RBs.
    let run = position_run(&picks, 6, 4).expect("run");
    assert_eq!((run.position.as_str(), run.count, run.window), ("RB", 4, 6));
    assert_eq!(position_run(&picks, 6, 5), None);
    // Nothing before the window counts: only the first pick is a WR.
    assert_eq!(
        position_run(&picks, 4, 2).map(|r| r.position),
        Some("RB".into())
    );
}
