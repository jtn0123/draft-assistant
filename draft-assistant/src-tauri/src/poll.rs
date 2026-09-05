//! The decisions each poll tick makes, separated from the machinery that runs
//! them.
//!
//! The loops themselves live in the command layer, where they need Tauri's
//! managed state and event emitter. Everything they actually *decide* — has
//! anything changed, is it worth emitting, is the cached analysis still good,
//! how should a failure be recorded — lives here, where it can be tested
//! without a running app.

mod season_loop;

pub use season_loop::{season_tick, SeasonPollMemory, SeasonTick};

use crate::engine::{now_secs, LoadedLeague};
use crate::season::{SeasonAnalysis, SeasonView};
use crate::season_live::LiveGame;
use crate::sleeper::Pick;
use serde::Serialize;
use std::collections::hash_map::DefaultHasher;
use std::hash::{Hash, Hasher};

/// What the health badge listens for: the three facts about the last tick.
#[derive(Debug, Clone, Serialize)]
pub struct PollHealth {
    pub last_success_at: Option<u64>,
    pub consecutive_failures: u32,
    pub last_error: Option<String>,
}

/// The draft poller keeps its record on the league it is watching, so its
/// report is read straight back off there.
pub fn poll_health(loaded: &LoadedLeague) -> PollHealth {
    PollHealth {
        last_success_at: loaded.poll_last_success_at,
        consecutive_failures: loaded.poll_consecutive_failures,
        last_error: loaded.poll_last_error.clone(),
    }
}

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

/// Whether a poller's requests are getting through, and why not when they are
/// not.
///
/// Both pollers keep the same three facts. The draft poller stores them on the
/// league it is watching (they ride along in `DataHealth`); the season poller
/// has no such home, so it keeps one of these in the loop itself. The rule for
/// updating them lives here once, in `record`, rather than in either loop.
#[derive(Debug, Default, Clone)]
pub struct PollHealthMemory {
    last_success_at: Option<u64>,
    consecutive_failures: u32,
    last_error: Option<String>,
}

impl PollHealthMemory {
    /// A tick with no errors resets the failure count; a tick with errors adds
    /// to it and keeps every reason, so "failing for 3 tries because X" is
    /// available. A failure never moves the last-success time.
    pub fn record(&mut self, errors: &[String]) {
        if errors.is_empty() {
            self.last_success_at = Some(now_secs());
            self.consecutive_failures = 0;
            self.last_error = None;
        } else {
            self.consecutive_failures = self.consecutive_failures.saturating_add(1);
            self.last_error = Some(errors.join("; "));
        }
    }

    /// The same three facts in the shape the frontend already listens for.
    pub fn report(&self) -> PollHealth {
        PollHealth {
            last_success_at: self.last_success_at,
            consecutive_failures: self.consecutive_failures,
            last_error: self.last_error.clone(),
        }
    }
}

/// Record a tick's outcome on the league so the health badge can report it.
///
/// The draft poller's spelling of `PollHealthMemory::record`: same rule, but
/// reading and writing the fields the draft view already carries.
pub fn record_poll_outcome(loaded: &mut LoadedLeague, errors: &[String]) {
    let mut health = PollHealthMemory {
        last_success_at: loaded.poll_last_success_at,
        consecutive_failures: loaded.poll_consecutive_failures,
        last_error: loaded.poll_last_error.clone(),
    };
    health.record(errors);
    loaded.poll_last_success_at = health.last_success_at;
    loaded.poll_consecutive_failures = health.consecutive_failures;
    loaded.poll_last_error = health.last_error;
}

/// A cheap stand-in for the scoreboard behind the totals: every game's state,
/// clock and score, and who is on the field for each of them.
///
/// The totals alone were not enough. Sunday morning they are 0 - 0 and stay
/// 0 - 0 through every kickoff, so the screen froze exactly when it had the
/// most to say: games going live, the clock running, a starter swapped in.
/// Hashing the games costs one pass over a couple of dozen rows per tick.
fn games_signature(games: &[LiveGame]) -> u64 {
    let mut hasher = DefaultHasher::new();
    for game in games {
        game.game_id.hash(&mut hasher);
        (game.state as u8).hash(&mut hasher);
        game.status.hash(&mut hasher);
        game.away_score.hash(&mut hasher);
        game.home_score.hash(&mut hasher);
        for chip in &game.chips {
            chip.player_id.hash(&mut hasher);
            chip.slot.hash(&mut hasher);
            (chip.state as u8).hash(&mut hasher);
        }
    }
    hasher.finish()
}

/// Suppresses season-updated events identical to the last one. The view is
/// large and the whole panel re-renders on every event, so emitting an
/// unchanged one is pure cost.
#[derive(Debug, Default)]
pub struct LiveEmitGate {
    /// Points as hundredths (floats are not worth comparing exactly), the
    /// scoreboard they were counted off, and which build of the analysis they
    /// were carried beside.
    last: Option<(u64, u64, u64, u64)>,
}

impl LiveEmitGate {
    /// `analysis` is [`AnalysisCache::generation`]: it steps every time the
    /// expensive half of the view is rebuilt.
    ///
    /// Without it a midweek rebuild was computed and then thrown away. Waivers
    /// clear on a Tuesday morning with every game 0 - 0 and every scoreboard
    /// row identical, so the gate saw nothing move and the fresh waiver
    /// targets, trade ideas and playoff odds never reached the screen — until
    /// something scored, which on a Tuesday is never.
    pub(crate) fn should_emit(
        &mut self,
        my_points: f64,
        opp_points: f64,
        games: &[LiveGame],
        analysis: u64,
    ) -> bool {
        let signature = (
            (my_points * 100.0).round() as u64,
            (opp_points * 100.0).round() as u64,
            games_signature(games),
            analysis,
        );
        if self.last == Some(signature) {
            return false;
        }
        self.last = Some(signature);
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
    generation: u64,
}

impl AnalysisCache {
    pub fn new(rebuild_every: u32) -> Self {
        Self {
            held: None,
            ticks: 0,
            rebuild_every: rebuild_every.max(1),
            generation: 0,
        }
    }

    /// What to hand `build_season_view_cached`, or `None` to build it fresh.
    pub fn get(&self) -> Option<&SeasonAnalysis> {
        self.held.as_ref()
    }

    /// Which build of the analysis is being held. Steps every time a fresh one
    /// is taken, which is what tells the emit gate that a view carries
    /// something new even when nobody has scored.
    pub fn generation(&self) -> u64 {
        self.generation
    }

    /// Throw the held analysis away, so the next tick builds a fresh one.
    /// Used when the ground has moved under it — a new week, most of all.
    pub fn invalidate(&mut self) {
        self.held = None;
    }

    /// Take the reusable parts out of a freshly built view, and count the tick
    /// so the cache expires on schedule.
    pub fn observe(&mut self, view: &SeasonView) {
        if self.held.is_none() {
            self.held = Some(SeasonAnalysis::of(view));
            self.generation = self.generation.wrapping_add(1);
        }
        self.ticks = self.ticks.saturating_add(1);
        if self.ticks.is_multiple_of(self.rebuild_every) {
            self.held = None;
        }
    }
}

#[cfg(test)]
#[path = "poll_decision_tests.rs"]
mod tests;
