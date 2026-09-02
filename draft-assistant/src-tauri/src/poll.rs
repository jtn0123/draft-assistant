//! The decisions each poll tick makes, separated from the machinery that runs
//! them.
//!
//! The loops themselves live in the command layer, where they need Tauri's
//! managed state and event emitter. Everything they actually *decide* — has
//! anything changed, is it worth emitting, is the cached analysis still good,
//! how should a failure be recorded — lives here, where it can be tested
//! without a running app.

use crate::engine::{now_secs, AppConfig, LoadedLeague};
use crate::season::{build_season_view_cached, SeasonAnalysis, SeasonView};
use crate::season_engine::{LoadedSeason, SeasonLoader};
use crate::sleeper::Pick;
use serde::Serialize;
use std::collections::hash_map::DefaultHasher;
use std::hash::{Hash, Hasher};
use tokio::sync::Mutex;

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

/// Suppresses season-updated events whose scores are identical to the last
/// one. The view is large and the whole panel re-renders on every event, so
/// emitting an unchanged one is pure cost.
#[derive(Debug, Default)]
pub struct LiveEmitGate {
    /// Points as hundredths, because floats are not worth comparing exactly.
    last_totals: Option<(u64, u64)>,
}

impl LiveEmitGate {
    fn should_emit(&mut self, my_points: f64, opp_points: f64) -> bool {
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
        if self.ticks.is_multiple_of(self.rebuild_every) {
            self.held = None;
        }
    }
}

/// What the season poller remembers between ticks: whether it is getting
/// through, what it last emitted, and the analysis it is reusing.
#[derive(Debug)]
pub struct SeasonPollMemory {
    health: PollHealthMemory,
    gate: LiveEmitGate,
    analysis: AnalysisCache,
}

impl SeasonPollMemory {
    /// `rebuild_every` is how many ticks the cached analysis is reused for.
    pub fn new(rebuild_every: u32) -> Self {
        Self {
            health: PollHealthMemory::default(),
            gate: LiveEmitGate::default(),
            analysis: AnalysisCache::new(rebuild_every),
        }
    }
}

/// What one season tick decided the app should be told.
#[derive(Debug, Default)]
pub struct SeasonTick {
    /// The view worth emitting, or `None` when the scores have not moved.
    pub view: Option<SeasonView>,
    /// How the refresh went, or `None` when there was nothing to refresh — no
    /// league open yet, or the season not loaded. Neither is the feed failing,
    /// so neither should be reported as one.
    pub health: Option<PollHealth>,
}

/// One turn of the season poll loop: refresh the live slice, note whether that
/// worked, and rebuild the view if the scores moved.
///
/// The loop around this lives in the command layer because it needs Tauri's
/// event emitter; everything it decides lives here, where a test can drive it
/// with a loader that fails on demand.
pub async fn season_tick<E: SeasonLoader>(
    engine: &E,
    loaded_ref: &Mutex<Option<LoadedLeague>>,
    season_ref: &Mutex<Option<LoadedSeason>>,
    config_ref: &Mutex<AppConfig>,
    memory: &mut SeasonPollMemory,
) -> SeasonTick {
    let league_id = {
        let loaded = loaded_ref.lock().await;
        loaded.as_ref().map(|l| l.league.league_id.clone())
    };
    let Some(league_id) = league_id else {
        return SeasonTick::default();
    };

    let watching = {
        let season = season_ref.lock().await;
        let Some(season) = season.as_ref() else {
            return SeasonTick::default();
        };
        (season.season, season.week)
    };
    // The three requests run with nothing locked. Each has an eight-second
    // timeout and retries, so holding `season` across them stalled every
    // command that needs it and queued the next tick behind this one.
    let fetched = engine.fetch_live(&league_id, watching.0, watching.1).await;
    let mut errors = Vec::new();
    {
        let mut season = season_ref.lock().await;
        let Some(season) = season.as_mut() else {
            return SeasonTick::default();
        };
        if let Err(error) = fetched.apply(season, now_secs()) {
            errors.push(error);
        }
    }
    memory.health.record(&errors);
    let health = Some(memory.health.report());
    if !errors.is_empty() {
        return SeasonTick { view: None, health };
    }

    // Locks are taken loaded -> season -> config here, the same order as
    // everywhere else that needs more than one of them.
    let loaded = loaded_ref.lock().await;
    let season = season_ref.lock().await;
    let config = config_ref.lock().await;
    let (Some(loaded), Some(season)) = (loaded.as_ref(), season.as_ref()) else {
        return SeasonTick { view: None, health };
    };
    let view = build_season_view_cached(
        loaded,
        season,
        config.my_user_id.as_deref(),
        memory.analysis.get(),
    );
    memory.analysis.observe(&view);
    let moved = memory.gate.should_emit(
        view.live.totals.my_live_points,
        view.live.totals.opp_live_points,
    );
    SeasonTick {
        view: moved.then_some(view),
        health,
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
                is_keeper: None,
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
