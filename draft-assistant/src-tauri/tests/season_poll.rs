//! One turn of the season poll loop, driven against a loader that can be made
//! to fail. The loop this backs used to throw refresh errors away, so Sleeper
//! could be down all Sunday with nothing on screen to say so.

mod common;

use draft_assistant_lib::engine::LoadedLeague;
use draft_assistant_lib::poll::{season_tick, SeasonPollMemory, SeasonTick};
use draft_assistant_lib::season_engine::{LoadedSeason, SeasonLoader};
use draft_assistant_lib::sleeper::League;
use std::cell::Cell;
use tokio::sync::Mutex;

/// The message a real total outage produces: every endpoint named, so the
/// screen can repeat it back verbatim.
const OUTAGE: &str = "matchups: request failed; scores: request failed; rosters: request failed";

/// A season loader whose live refresh fails whenever `failing` is set.
struct Flaky {
    failing: Cell<bool>,
}

impl SeasonLoader for Flaky {
    async fn load_season(
        &self,
        _league: &League,
        _my_user_id: Option<&str>,
        _force: bool,
    ) -> Result<LoadedSeason, String> {
        Err("the poller never loads a season".to_string())
    }

    async fn refresh_live(
        &self,
        season: &mut LoadedSeason,
        _league_id: &str,
    ) -> Result<(), String> {
        if self.failing.get() {
            return Err(OUTAGE.to_string());
        }
        // A real refresh moves the staleness clock and the live totals.
        season.fetched_at += 30;
        Ok(())
    }
}

/// The three pieces of app state a tick reads, plus the loader driving it.
struct Harness {
    engine: Flaky,
    loaded: Mutex<Option<LoadedLeague>>,
    season: Mutex<Option<LoadedSeason>>,
    config: Mutex<draft_assistant_lib::engine::AppConfig>,
    memory: SeasonPollMemory,
}

impl Harness {
    fn new() -> Self {
        let (loaded, season, config) = common::fixture();
        Self {
            engine: Flaky {
                failing: Cell::new(false),
            },
            loaded: Mutex::new(Some(loaded)),
            season: Mutex::new(Some(season)),
            config: Mutex::new(config),
            memory: SeasonPollMemory::new(20),
        }
    }

    async fn tick(&mut self) -> SeasonTick {
        season_tick(
            &self.engine,
            &self.loaded,
            &self.season,
            &self.config,
            &mut self.memory,
        )
        .await
    }
}

#[tokio::test]
async fn a_failing_refresh_is_reported_instead_of_swallowed() {
    let mut harness = Harness::new();
    harness.engine.failing.set(true);

    let tick = harness.tick().await;
    let health = tick
        .health
        .expect("a failed refresh must report its health");
    assert_eq!(health.consecutive_failures, 1);
    assert_eq!(
        health.last_error.as_deref(),
        Some(OUTAGE),
        "the reason must survive to the screen"
    );
    assert!(
        health.last_success_at.is_none(),
        "nothing has succeeded yet, so there is no last-success time"
    );
    assert!(
        tick.view.is_none(),
        "a refresh that failed has no new scores to show"
    );
}

#[tokio::test]
async fn every_failed_try_in_a_row_is_counted() {
    let mut harness = Harness::new();
    harness.engine.failing.set(true);

    for expected in 1..=3u32 {
        let health = harness.tick().await.health.expect("health every tick");
        assert_eq!(
            health.consecutive_failures, expected,
            "the run of failures must keep counting"
        );
    }
}

#[tokio::test]
async fn a_working_refresh_clears_the_failure_run() {
    let mut harness = Harness::new();
    harness.engine.failing.set(true);
    harness.tick().await;
    harness.tick().await;

    harness.engine.failing.set(false);
    let tick = harness.tick().await;
    let health = tick.health.expect("a good tick reports health too");
    assert_eq!(health.consecutive_failures, 0);
    assert!(
        health.last_error.is_none(),
        "the old reason must be dropped"
    );
    assert!(
        health.last_success_at.is_some(),
        "a working refresh sets the last-success time"
    );
    assert!(
        tick.view.is_some(),
        "the first working tick must push a view"
    );

    // And a failure afterwards keeps the success it already recorded.
    harness.engine.failing.set(true);
    let after = harness.tick().await.health.expect("health");
    assert_eq!(after.consecutive_failures, 1);
    assert_eq!(after.last_success_at, health.last_success_at);
}

#[tokio::test]
async fn nothing_to_poll_is_not_reported_as_a_failure() {
    let mut harness = Harness::new();
    harness.engine.failing.set(true);

    *harness.season.lock().await = None;
    let tick = harness.tick().await;
    assert!(
        tick.health.is_none(),
        "no season loaded is not the feed failing"
    );

    *harness.loaded.lock().await = None;
    let tick = harness.tick().await;
    assert!(
        tick.health.is_none(),
        "no league open is not the feed failing"
    );
}

#[tokio::test]
async fn scores_that_have_not_moved_do_not_push_a_view() {
    let mut harness = Harness::new();
    assert!(
        harness.tick().await.view.is_some(),
        "the first view must be sent"
    );
    let second = harness.tick().await;
    assert!(
        second.view.is_none(),
        "an unchanged score is not worth re-rendering the whole screen"
    );
    assert_eq!(
        second
            .health
            .expect("health is reported even when nothing moved")
            .consecutive_failures,
        0
    );
}
