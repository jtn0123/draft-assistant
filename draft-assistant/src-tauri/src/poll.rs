//! The decisions each poll tick makes, separated from the machinery that runs
//! them.
//!
//! The loops themselves live in the command layer, where they need Tauri's
//! managed state and event emitter. Everything they actually *decide* — has
//! anything changed, is it worth emitting, is the cached analysis still good,
//! how should a failure be recorded — lives here, where it can be tested
//! without a running app.

use crate::engine::{now_secs, LoadedLeague};
use crate::season::{SeasonAnalysis, SeasonView};
use crate::sleeper::Pick;
use std::collections::hash_map::DefaultHasher;
use std::hash::{Hash, Hasher};

/// A cheap stand-in for the whole pick list: how many there are, and a hash of
/// which player sits at which pick number.
///
/// Counting alone missed the case that actually bites — a commissioner editing
/// or replacing a pick, which leaves the count untouched but changes the board
/// under the user. Hashing the ids costs a single pass over a list that never
/// exceeds a couple of hundred entries, once per poll tick.
type PicksSignature = (usize, u64);

fn picks_signature(picks: &[Pick]) -> PicksSignature {
    let mut hasher = DefaultHasher::new();
    for pick in picks {
        pick.pick_no.hash(&mut hasher);
        pick.player_id.hash(&mut hasher);
    }
    (picks.len(), hasher.finish())
}

/// What the draft poller remembers between ticks so it can tell a real change
/// from another identical response.
#[derive(Debug, Default)]
pub struct DraftPollMemory {
    last_picks: Option<PicksSignature>,
    last_status: String,
}

impl DraftPollMemory {
    /// True when the picks differ from the last tick — a new pick, a removed
    /// one, or the same number of picks with a different player in one of
    /// them. The first tick counts as a change, so the UI gets its initial
    /// state.
    pub fn picks_changed(&mut self, picks: &[Pick]) -> bool {
        let signature = picks_signature(picks);
        if self.last_picks == Some(signature) {
            return false;
        }
        self.last_picks = Some(signature);
        true
    }

    /// True when the draft's status string moved (`pre_draft` -> `drafting` ->
    /// `complete`), which changes what the screen shows even with no new pick.
    pub fn status_changed(&mut self, status: &str) -> bool {
        if self.last_status == status {
            return false;
        }
        self.last_status = status.to_string();
        true
    }
}

/// Record a tick's outcome on the league so the health badge can report it.
///
/// A tick with no errors resets the failure count; a tick with errors adds to
/// it and keeps the reason, so "stale for 3 tries because X" is available.
pub fn record_poll_outcome(loaded: &mut LoadedLeague, errors: &[String]) {
    if errors.is_empty() {
        loaded.poll_last_success_at = Some(now_secs());
        loaded.poll_consecutive_failures = 0;
        loaded.poll_last_error = None;
    } else {
        loaded.poll_consecutive_failures = loaded.poll_consecutive_failures.saturating_add(1);
        loaded.poll_last_error = Some(errors.join("; "));
    }
}

/// Suppresses season-updated events whose scores are identical to the last
/// one. The view is large and the whole panel re-renders on every event, so
/// emitting an unchanged one is pure cost.
#[derive(Debug, Default)]
pub struct LiveEmitGate {
    /// Points as hundredths, because floats are not worth comparing exactly.
    last_totals: Option<(u64, u64)>,
}

impl LiveEmitGate {
    pub fn should_emit(&mut self, my_points: f64, opp_points: f64) -> bool {
        let totals = (
            (my_points * 100.0).round() as u64,
            (opp_points * 100.0).round() as u64,
        );
        if self.last_totals == Some(totals) {
            return false;
        }
        self.last_totals = Some(totals);
        true
    }
}

/// Holds the expensive half of a season view between ticks.
///
/// Playoff odds, waiver targets and trade ideas cannot change because someone
/// scored, so the poller computes them once and reuses them. They are dropped
/// every `rebuild_every` ticks so a waiver claim or a trade elsewhere in the
/// league still works its way in.
#[derive(Debug)]
pub struct AnalysisCache {
    held: Option<SeasonAnalysis>,
    ticks: u32,
    rebuild_every: u32,
}

impl AnalysisCache {
    pub fn new(rebuild_every: u32) -> Self {
        Self {
            held: None,
            ticks: 0,
            rebuild_every: rebuild_every.max(1),
        }
    }

    /// What to hand `build_season_view_cached`, or `None` to build it fresh.
    pub fn get(&self) -> Option<&SeasonAnalysis> {
        self.held.as_ref()
    }

    /// Take the reusable parts out of a freshly built view, and count the tick
    /// so the cache expires on schedule.
    pub fn observe(&mut self, view: &SeasonView) {
        if self.held.is_none() {
            self.held = Some(SeasonAnalysis::of(view));
        }
        self.ticks = self.ticks.saturating_add(1);
        if self.ticks % self.rebuild_every == 0 {
            self.held = None;
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// `n` picks, each of player `id{i}` unless `swap` renames one of them.
    fn picks(n: u32, swap: Option<(u32, &str)>) -> Vec<Pick> {
        (1..=n)
            .map(|pick_no| Pick {
                round: 1,
                pick_no,
                draft_slot: pick_no,
                player_id: match swap {
                    Some((at, id)) if at == pick_no => id.to_string(),
                    _ => format!("id{pick_no}"),
                },
                picked_by: None,
                metadata: None,
            })
            .collect()
    }

    #[test]
    fn the_first_tick_always_counts_as_a_change() {
        let mut memory = DraftPollMemory::default();
        assert!(
            memory.picks_changed(&picks(0, None)),
            "the initial state must reach the UI"
        );
        assert!(memory.status_changed("pre_draft"));
    }

    #[test]
    fn an_identical_response_is_not_a_change() {
        let mut memory = DraftPollMemory::default();
        memory.picks_changed(&picks(26, None));
        assert!(!memory.picks_changed(&picks(26, None)));
        assert!(
            memory.picks_changed(&picks(27, None)),
            "a new pick is a change"
        );
        assert!(!memory.picks_changed(&picks(27, None)));
    }

    #[test]
    fn a_commissioner_swapping_a_pick_is_a_change_at_the_same_count() {
        // The bug this guards: pick 14 is edited to a different player, the
        // count never moves, and the board silently keeps the old name.
        let mut memory = DraftPollMemory::default();
        assert!(memory.picks_changed(&picks(26, None)));
        assert!(
            memory.picks_changed(&picks(26, Some((14, "someone-else")))),
            "an edited pick must reach the UI even at an unchanged count"
        );
        assert!(!memory.picks_changed(&picks(26, Some((14, "someone-else")))));
        assert!(
            memory.picks_changed(&picks(26, None)),
            "and undoing the edit is a change too"
        );
    }

    #[test]
    fn a_status_move_is_a_change_even_with_no_new_pick() {
        let mut memory = DraftPollMemory::default();
        memory.status_changed("drafting");
        assert!(!memory.status_changed("drafting"));
        assert!(memory.status_changed("complete"));
    }

    #[test]
    fn scores_that_have_not_moved_do_not_emit() {
        let mut gate = LiveEmitGate::default();
        assert!(gate.should_emit(101.4, 98.2), "the first view must be sent");
        assert!(!gate.should_emit(101.4, 98.2));
        // Below a hundredth of a point is not a score change.
        assert!(!gate.should_emit(101.4001, 98.2001));
        assert!(gate.should_emit(101.5, 98.2));
        assert!(
            gate.should_emit(101.5, 98.3),
            "the opponent moving counts too"
        );
    }
}
