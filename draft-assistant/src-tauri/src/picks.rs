//! The pick list itself: merging the feed with manual picks, where the draft
//! is up to, which picks are keepers, and the fingerprint the poll loop uses
//! to notice a change. Lifted out of `view.rs` for the 500-line cap.

use crate::sleeper::Pick;
use std::collections::HashSet;

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

/// What the poll loop compares between polls to decide whether the UI needs
/// a fresh view. Must change whenever anything the view renders from the
/// draft feed changes — not just the pick count.
pub fn poll_fingerprint(picks: &[Pick], draft: &crate::sleeper::Draft) -> u64 {
    use std::hash::{Hash, Hasher};
    let mut hasher = std::collections::hash_map::DefaultHasher::new();
    for pick in picks {
        (pick.pick_no, pick.draft_slot, pick.player_id.as_str()).hash(&mut hasher);
    }
    draft.status.hash(&mut hasher);
    draft.last_picked.hash(&mut hasher);
    hasher.finish()
}

/// Merge API picks with manual fallback picks. API picks are authoritative;
/// a manual pick survives only where the API has not filled that pick number.
///
/// Keyed on the number rather than "beyond the highest API pick": a keeper
/// league arrives with picks scattered all over the board (this league opens
/// with keepers at 11, 14, 20 … 177), and the old rule silently threw away
/// every manual pick below the last of them.
pub fn merged_picks(api: &[Pick], manual: &[Pick]) -> Vec<Pick> {
    let taken: std::collections::HashSet<u32> = api.iter().map(|p| p.pick_no).collect();
    let mut picks = api.to_vec();
    for m in manual {
        if !taken.contains(&m.pick_no) {
            picks.push(m.clone());
        }
    }
    picks.sort_by_key(|p| p.pick_no);
    picks
}

/// The lowest pick number nobody has filled yet, or `None` once the board is
/// full. Counting picks instead would put a keeper league several rounds ahead
/// of itself before the draft even starts.
pub fn next_open_pick(picks: &[Pick], teams: u32, rounds: u32) -> Option<u32> {
    let made: std::collections::HashSet<u32> = picks.iter().map(|p| p.pick_no).collect();
    (1..=teams.saturating_mul(rounds)).find(|pick| !made.contains(pick))
}

/// Which picks are keepers, judged by position rather than by Sleeper's
/// flag: anything flagged, plus anything already in the book at or beyond
/// the next open pick — a pick the draft has not reached can only be a
/// keeper. Union this into `LoadedLeague::keeper_pick_nos` whenever picks
/// arrive so the judgement survives the draft passing the slot.
pub fn keeper_pick_nos(picks: &[Pick], teams: u32, rounds: u32) -> HashSet<u32> {
    let open = next_open_pick(picks, teams, rounds).unwrap_or(u32::MAX);
    picks
        .iter()
        .filter(|p| p.is_keeper == Some(true) || p.pick_no >= open)
        .map(|p| p.pick_no)
        .collect()
}

/// The one thing the recommendation card is too quiet about: a dedicated
/// starting slot (not a flex — anyone can fill those) with nobody in it,
/// and no more picks than it takes to fill the holes. On draft night the
/// user finished with an empty DEF and three backs who never start; the
/// card had said "DEF" in one line among six.
pub fn starter_alert(open_starters: &[(String, u32)], picks_left: usize) -> Option<String> {
    let mut holes: Vec<(String, u32)> = open_starters
        .iter()
        .filter(|(slot, n)| *n > 0 && crate::roster::RosterRules::flex_eligible(slot).is_none())
        .cloned()
        .collect();
    if holes.is_empty() {
        return None;
    }
    let needed: u32 = holes.iter().map(|(_, n)| n).sum();
    // Room for one flier beyond the holes is fine; less than that is not.
    if picks_left as u32 > needed + 1 {
        return None;
    }
    holes.sort();
    let list: Vec<String> = holes
        .iter()
        .map(|(slot, n)| {
            if *n > 1 {
                format!("{slot} ×{n}")
            } else {
                slot.clone()
            }
        })
        .collect();
    Some(format!(
        "{} still empty with {} pick{} left",
        list.join(", "),
        picks_left,
        if picks_left == 1 { "" } else { "s" }
    ))
}

/// `starter_alert` for my roster, only while the draft is live.
pub fn alert_for(
    roster: Option<&crate::draft::TeamRoster>,
    drafting: bool,
    picks_left: usize,
) -> Option<String> {
    roster
        .filter(|_| drafting)
        .and_then(|r| starter_alert(&r.open_starters, picks_left))
}

#[cfg(test)]
mod starter_alert_tests {
    use super::starter_alert;

    fn open(slots: &[(&str, u32)]) -> Vec<(String, u32)> {
        slots.iter().map(|(s, n)| (s.to_string(), *n)).collect()
    }

    #[test]
    fn an_empty_defense_with_one_pick_left_is_shouted() {
        assert_eq!(
            starter_alert(&open(&[("DEF", 1)]), 1).as_deref(),
            Some("DEF still empty with 1 pick left")
        );
        assert_eq!(
            starter_alert(&open(&[("DEF", 1), ("TE", 1)]), 3).as_deref(),
            Some("DEF, TE still empty with 3 picks left")
        );
    }

    #[test]
    fn flex_slots_and_plenty_of_picks_are_not_alarms() {
        assert_eq!(starter_alert(&open(&[("FLEX", 2)]), 1), None);
        assert_eq!(starter_alert(&open(&[("DEF", 1)]), 5), None);
        assert_eq!(starter_alert(&[], 1), None);
    }
}
