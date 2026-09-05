//! The season poll loop, and the generation bookkeeping that decides which
//! loop is the live one.
//!
//! Split out of `commands_season.rs`, which is near the line cap, and kept
//! apart from the commands themselves so the start/stop ordering rules can be
//! tested without a running Tauri app.

use crate::commands_draft::tick::backoff_secs;
use crate::poll::{season_tick, SeasonPollMemory};
use crate::state::{AppState, CachedSeasonView};
use std::sync::atomic::{AtomicU64, Ordering};
use std::sync::Arc;
use tauri::Emitter;

/// How many polls to reuse the cached analysis before rebuilding it. At the
/// default 30s interval that is roughly ten minutes.
const ANALYSIS_EVERY: u32 = 20;

/// Claim the next generation for a loop that is about to start.
///
/// Every loop carries the number it was handed and stops the moment the shared
/// counter moves past it, so starting a second loop retires the first without
/// either of them needing to know about the other.
pub(crate) fn begin(generation: &AtomicU64) -> u64 {
    generation.fetch_add(1, Ordering::SeqCst) + 1
}

/// Retire whichever loop is running.
///
/// A bump rather than a sticky "stop" flag, which is the bug this replaces.
/// The flag stayed set until something cleared it, so a stop whose effect was
/// observed after the user had already restarted polling killed the *new*
/// loop and left the screen with no live scoring and nothing saying so. A
/// generation bump only invalidates the generations that already exist: a
/// start that comes afterwards takes a number the stop never touched.
pub(crate) fn cancel(generation: &AtomicU64) {
    generation.fetch_add(1, Ordering::SeqCst);
}

/// True while `mine` is still the generation being polled.
pub(crate) fn is_live(generation: &AtomicU64, mine: u64) -> bool {
    generation.load(Ordering::SeqCst) == mine
}

/// Spawn the loop that polls live scoring for as long as `generation` stands.
pub(crate) fn spawn<R: tauri::Runtime>(app: tauri::AppHandle<R>, state: &AppState, interval: u64) {
    let generation = begin(&state.season_generation);
    state.season_polling.store(true, Ordering::SeqCst);

    let engine = state.engine.clone();
    let loaded_ref = state.loaded.clone();
    let season_ref = state.season.clone();
    let config_ref = state.config.clone();
    let last_view = state.last_season_view.clone();
    let season_generation = state.season_generation.clone();

    tauri::async_runtime::spawn(async move {
        let mut memory = SeasonPollMemory::new(ANALYSIS_EVERY);
        loop {
            if !is_live(&season_generation, generation) {
                break;
            }
            let tick =
                season_tick(&*engine, &loaded_ref, &season_ref, &config_ref, &mut memory).await;
            // Health first: when a refresh fails there is no view to send, and
            // the screen still has to hear that the attempt was made and lost.
            let failures = tick.health.as_ref().map_or(0, |h| h.consecutive_failures);
            if let Some(health) = &tick.health {
                app.emit("season-poll-health", health).ok();
                crate::companion::publish(&app, "season-poll-health", health);
            }
            if let Some(view) = tick.view {
                let view = Arc::new(view);
                app.emit("season-updated", view.as_ref()).ok();
                crate::companion::publish(&app, "season-updated", view.as_ref());
                // Chat answers from the last built view rather than paying for
                // a whole build of its own. Only the season screen used to
                // leave one here, so a question asked while the poller was the
                // only thing running rebuilt everything from scratch.
                *last_view.lock().await = Some(CachedSeasonView::new(view));
            }
            // The same backoff the draft loop uses. A season feed that has
            // gone away — no wifi, a bad Sunday at Sleeper — used to be asked
            // again every thirty seconds forever, which is the worst thing to
            // do to a service already struggling and burns battery for
            // nothing. One success puts the cadence straight back.
            let wait = backoff_secs(interval, failures);
            tokio::time::sleep(std::time::Duration::from_secs(wait)).await;
        }
    });
}

#[cfg(test)]
mod tests {
    use super::*;

    /// The bug: stopping set a sticky flag, so a stop observed after the user
    /// had restarted polling killed the loop that had just been started.
    #[test]
    fn a_stop_cannot_reach_forward_and_kill_a_later_start() {
        let generation = AtomicU64::new(0);
        let first = begin(&generation);
        cancel(&generation);
        let second = begin(&generation);

        assert!(
            !is_live(&generation, first),
            "the stopped loop kept running"
        );
        assert!(
            is_live(&generation, second),
            "the stop killed the loop started after it"
        );
    }

    /// Asking twice replaces the running loop rather than doubling the poll
    /// rate, which is what makes `start_season_polling` safe to call again.
    #[test]
    fn a_second_start_retires_the_first_loop() {
        let generation = AtomicU64::new(0);
        let first = begin(&generation);
        let second = begin(&generation);
        assert!(!is_live(&generation, first));
        assert!(is_live(&generation, second));

        cancel(&generation);
        assert!(!is_live(&generation, second));
    }

    /// The failure count the loop backs off on is the one the health report
    /// already carries, so the two can never disagree about how bad it is.
    #[test]
    fn the_wait_grows_with_the_failure_count_and_snaps_back_on_success() {
        assert_eq!(backoff_secs(30, 0), 30);
        assert_eq!(backoff_secs(30, 1), 60);
        assert_eq!(backoff_secs(30, 5), 60);
    }
}
