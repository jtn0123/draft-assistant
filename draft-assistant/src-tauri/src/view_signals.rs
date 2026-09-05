//! The small derived signals in a draft view: my validated seat, the pick
//! clock's deadline, and the positional run.
//!
//! Each is a pure function of a couple of raw Sleeper fields, and each is a
//! place this app has been wrong before — a slot outside the league, a
//! deadline on a draft that has not started, a run counted past its window.
//! Here they can be read, and tested, without the several hundred lines of
//! view assembly they are called from.

use crate::roster::RosterRules;
use crate::view_types::PositionRun;
use std::collections::{BTreeSet, HashMap, HashSet};

pub(crate) fn validated_slot(slot: Option<u32>, teams: u32) -> (Option<u32>, Option<String>) {
    match slot {
        Some(value) if !(1..=teams).contains(&value) => (
            None,
            Some(format!(
                "your draft slot {value} is outside the valid range 1..={teams}"
            )),
        ),
        _ => (slot, None),
    }
}

/// When the current pick's timer runs out, from Sleeper's `last_picked`
/// stamp, the draft's scheduled `start_time`, and its `pick_timer`. Only
/// meaningful mid-draft.
///
/// `last_picked` alone is not enough twice over. Pick one has no last pick at
/// all, so the very first clock of every draft was blank until somebody
/// selected. And a keeper league carries a `last_picked` from the evening the
/// keepers were entered, days or weeks before anybody is on the clock, which
/// put the opening deadline in the past and showed 0:00. Both stamps are
/// epoch milliseconds, so the clock starts from whichever of them is later.
pub(crate) fn clock_deadline_ms(
    status: &str,
    last_picked: Option<u64>,
    pick_timer: Option<u32>,
    start_time: Option<i64>,
) -> Option<u64> {
    if status != "drafting" {
        return None;
    }
    let timer = u64::from(pick_timer.filter(|t| *t > 0)?) * 1000;
    let start = start_time
        .filter(|stamp| *stamp > 0)
        .map(|stamp| stamp as u64);
    let started_at = last_picked.into_iter().chain(start).max()?;
    Some(started_at + timer)
}

/// The pick a player's survival is judged at: my next turn after the window
/// the pick being made now belongs to.
///
/// A turn is not always one pick. At a snake turn I own two picks with
/// nothing in between; a traded pick can leave me three in a row; and in a
/// keeper league the picks between two of mine may already be in the book, so
/// nobody selects at those numbers at all. Every one of those cases used to
/// price survival against the second half of my own turn, which says
/// everybody survives — reading the one moment the board is *most* dangerous
/// as the safest.
///
/// So adjacency is measured over the picks that will actually be *made*: the
/// window runs on through every consecutive pick that is either mine or
/// already booked, and it is my turn after that window which counts.
///
/// `booked` is the keeper set — pick numbers already filled that nobody will
/// spend a selection on. MIRRORED by `survivalTargetPick` in Panels.tsx.
pub fn survival_target(
    my_next_picks: &[u32],
    current_pick: u32,
    is_my_pick: bool,
    booked: &HashSet<u32>,
) -> Option<u32> {
    let mine: BTreeSet<u32> = my_next_picks
        .iter()
        .copied()
        .filter(|pick| !is_my_pick || *pick != current_pick)
        .collect();
    mine.first()?;
    // The window starts at the pick being made now and runs on for as long as
    // the next number along is one nobody else gets to select at.
    let mut end = current_pick;
    while mine.contains(&(end + 1)) || booked.contains(&(end + 1)) {
        end += 1;
    }
    // My next real turn past the window — or, when the window is the last of
    // the draft, the final pick I hold inside it, because that is all there
    // is left to judge anything against.
    mine.range(end + 1..)
        .next()
        .or_else(|| mine.range(..=end).next_back())
        .copied()
}

/// Byes already stacked on the players who would actually *start*, keyed by
/// week.
///
/// The recommender's line for this says "shared with N of your starters", and
/// it meant it: a starting lineup with four men off in week 9 loses week 9,
/// while four bench bodies sharing a bye is not a problem at all. The count
/// fed to it was of the whole roster, so by round twelve it was reporting six
/// starters on a bye out of a lineup of nine, and docking every candidate who
/// shared that week for a clash that did not exist.
///
/// Who starts is read off the league's own slots, in the league's own order:
/// each dedicated slot takes an unused player at its position, then each flex
/// takes one of the players it accepts, earliest pick first — the same
/// best-available-first shape `RosterRules::open_starting_slots` fills with.
/// Approximate, because a real lineup is set weekly on form; but it is drawn
/// from the roster the user actually has rather than from all of it.
pub fn starter_byes<'a>(
    rules: &RosterRules,
    // (position, bye week), in the order the players were drafted.
    roster: impl IntoIterator<Item = (&'a str, Option<u32>)>,
) -> HashMap<u32, u32> {
    let mut unused: Vec<(&str, Option<u32>)> = roster.into_iter().collect();
    let mut byes: HashMap<u32, u32> = HashMap::new();
    let mut take = |eligible: &dyn Fn(&str) -> bool, byes: &mut HashMap<u32, u32>| {
        let Some(at) = unused.iter().position(|(pos, _)| eligible(pos)) else {
            return;
        };
        let (_, bye) = unused.remove(at);
        if let Some(bye) = bye {
            *byes.entry(bye).or_insert(0) += 1;
        }
    };
    // Dedicated slots first — a flex that swallowed the only tight end would
    // leave the TE slot claiming a player the roster does not have.
    for slot in rules.slots() {
        if RosterRules::is_non_starting(slot) || RosterRules::flex_eligible(slot).is_some() {
            continue;
        }
        let slot = slot.clone();
        take(&|position| position == slot, &mut byes);
    }
    for slot in rules.slots() {
        let Some(eligible) = RosterRules::flex_eligible(slot) else {
            continue;
        };
        take(&|position| eligible.contains(&position), &mut byes);
    }
    byes
}

/// How many recent picks a positional run is judged over, and how many of them
/// have to share a position for it to count as one.
pub(crate) const RUN_WINDOW: u32 = 6;
pub(crate) const RUN_MIN: u32 = 4;

/// The position taken at least `min_count` times in the last `window` picks.
pub fn position_run(positions: &[String], window: u32, min_count: u32) -> Option<PositionRun> {
    let mut counts: HashMap<&str, u32> = HashMap::new();
    for pos in positions.iter().rev().take(window as usize) {
        if !pos.is_empty() {
            *counts.entry(pos.as_str()).or_insert(0) += 1;
        }
    }
    counts
        .into_iter()
        .filter(|(_, c)| *c >= min_count)
        .max_by_key(|(_, c)| *c)
        .map(|(pos, count)| PositionRun {
            position: pos.to_string(),
            count,
            window,
        })
}

#[cfg(test)]
mod reliability_tests {
    use super::*;

    #[test]
    fn invalid_user_slots_are_rejected_before_roster_indexing() {
        assert_eq!(validated_slot(Some(0), 14).0, None);
        assert_eq!(validated_slot(Some(15), 14).0, None);
        assert_eq!(validated_slot(Some(2), 14).0, Some(2));
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
}
