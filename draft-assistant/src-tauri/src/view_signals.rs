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

/// Why this user has no seat in this draft, said in terms they can act on.
///
/// Only ever asked when the seat is unknown, and there are four quite
/// different reasons for that. They used to share one sentence: "set your
/// Sleeper username", shown to people who had set it hours ago, because a
/// draft whose order the platform has not published yet has no seat for
/// anybody, and neither has a league the user is not in.
pub(crate) fn seat_note(
    platform: &str,
    user_id: Option<&str>,
    order: Option<&HashMap<String, u32>>,
    slot_out_of_range: bool,
) -> String {
    if slot_out_of_range {
        return "Your saved draft slot is not one this league has.".into();
    }
    if platform == crate::view_types::YAHOO {
        return "Connect Yahoo to track your team.".into();
    }
    let Some(user_id) = user_id.filter(|id| !id.is_empty()) else {
        return "Set your Sleeper username to track your team.".into();
    };
    match order {
        // Before a draft opens Sleeper publishes no order at all, so nobody
        // has a seat yet and there is nothing for the user to fix.
        None => "The draft order has not been posted yet.".into(),
        Some(order) if order.is_empty() => "The draft order has not been posted yet.".into(),
        Some(order) if !order.contains_key(user_id) => "You are not in this league.".into(),
        // The order names this user, so the slot came back and this is not
        // called; kept total rather than panicking on a case that cannot
        // happen today and might tomorrow.
        Some(_) => "Your seat in this draft could not be worked out.".into(),
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
    // The same slots `open_starting_slots` builds for: an IDP slot is not one
    // of them, so a linebacker's bye is not a clash for the offensive lineup
    // the board is filling.
    for slot in rules.slots() {
        if !RosterRules::counts_as_open_starter(slot) || RosterRules::flex_eligible(slot).is_some()
        {
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
#[path = "view_signals_tests.rs"]
mod reliability_tests;
