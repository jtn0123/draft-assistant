//! The small derived signals in a draft view: my validated seat, the pick
//! clock's deadline, and the positional run.
//!
//! Each is a pure function of a couple of raw Sleeper fields, and each is a
//! place this app has been wrong before — a slot outside the league, a
//! deadline on a draft that has not started, a run counted past its window.
//! Here they can be read, and tested, without the several hundred lines of
//! view assembly they are called from.

use crate::view_types::PositionRun;
use std::collections::HashMap;

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
/// stamp and the draft's `pick_timer`. Only meaningful mid-draft.
pub(crate) fn clock_deadline_ms(
    status: &str,
    last_picked: Option<u64>,
    pick_timer: Option<u32>,
) -> Option<u64> {
    if status != "drafting" {
        return None;
    }
    Some(last_picked? + u64::from(pick_timer.filter(|t| *t > 0)?) * 1000)
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
            clock_deadline_ms("drafting", Some(1_000), Some(90)),
            Some(91_000)
        );
        assert_eq!(clock_deadline_ms("pre_draft", Some(1_000), Some(90)), None);
        assert_eq!(clock_deadline_ms("complete", Some(1_000), Some(90)), None);
        assert_eq!(clock_deadline_ms("drafting", None, Some(90)), None);
        assert_eq!(clock_deadline_ms("drafting", Some(1_000), None), None);
        assert_eq!(clock_deadline_ms("drafting", Some(1_000), Some(0)), None);
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
