//! One turn of the season poll loop, driven against a loader that can be made
//! to fail. The loop this backs used to throw refresh errors away, so Sleeper
//! could be down all Sunday with nothing on screen to say so.

mod common;
mod poll_support;

use draft_assistant_lib::engine::LoadedLeague;
use draft_assistant_lib::poll::{season_tick, SeasonPollMemory};
use draft_assistant_lib::season_engine::{LoadedSeason, SeasonLoader};
use draft_assistant_lib::season_history::{History, HistoryStore};
use draft_assistant_lib::season_refresh::{PlayerRefresh, PlayerRefreshData};
use draft_assistant_lib::season_sources::LiveFetch;
use draft_assistant_lib::sleeper::League;
use poll_support::Harness;

/// The message a real total outage produces: every endpoint named, so the
/// screen can repeat it back verbatim.
const OUTAGE: &str = "matchups: request failed; scores: request failed; rosters: request failed";
use std::cell::Cell;
use tokio::sync::Mutex;

#[tokio::test]
async fn a_failing_refresh_is_reported_instead_of_swallowed() {
    let mut harness = Harness::named("tick");
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
    let mut harness = Harness::named("tick");
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
    let mut harness = Harness::named("tick");
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
    let mut harness = Harness::named("tick");
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
    let mut harness = Harness::named("tick");
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

/// A loader that reports whether the season mutex was free while its three
/// live requests were in flight.
struct Watcher {
    season: std::sync::Arc<Mutex<Option<LoadedSeason>>>,
    free_during_fetch: Cell<Option<bool>>,
}

impl HistoryStore for Watcher {
    async fn record_history(&self, _loaded: &LoadedLeague, _season: &LoadedSeason) -> History {
        History::default()
    }
}

impl PlayerRefresh for Watcher {
    async fn refresh_players(&self, _season: u32) -> Option<PlayerRefreshData> {
        None
    }
}

impl SeasonLoader for Watcher {
    async fn load_season(
        &self,
        _league: &League,
        _my_user_id: Option<&str>,
        _force: bool,
    ) -> Result<LoadedSeason, String> {
        Err("the poller never loads a season".to_string())
    }

    async fn current_week(&self) -> Result<u32, String> {
        // The week this fixture is already on, so nothing reloads.
        Ok(self
            .season
            .try_lock()
            .map_or(1, |s| s.as_ref().map_or(1, |season| season.week)))
    }

    async fn fetch_live(&self, _league_id: &str, _season: u32, _week: u32) -> LiveFetch {
        self.free_during_fetch
            .set(Some(self.season.try_lock().is_ok()));
        LiveFetch {
            matchups: Ok(Vec::new()),
            scores: Ok(Vec::new()),
            rosters: Ok(Vec::new()),
        }
    }
}

/// The whole point of splitting fetch from apply. Three requests at an
/// eight-second timeout with retries behind them is tens of seconds, and the
/// season mutex used to be held for every bit of it — so `get_season`,
/// `load_season` and every chat question waited, and the next tick queued up
/// behind this one.
#[tokio::test]
async fn the_live_requests_run_with_the_season_mutex_free() {
    let (loaded, season, config) = common::fixture();
    let season = std::sync::Arc::new(Mutex::new(Some(season)));
    let engine = Watcher {
        season: season.clone(),
        free_during_fetch: Cell::new(None),
    };
    let loaded = Mutex::new(Some(loaded));
    let config = Mutex::new(config);
    let mut memory = SeasonPollMemory::new(20);

    season_tick(&engine, &loaded, &season, &config, &mut memory).await;

    assert_eq!(
        engine.free_during_fetch.get(),
        Some(true),
        "the season mutex was held across the network requests"
    );
}

/// The bug: `nfl_state` was asked once, at load, and never again. An app left
/// open across Tuesday's rollover kept scoring the previous week — the wrong
/// matchup, the wrong projections, the wrong scoreboard — with nothing on
/// screen to say so.
#[tokio::test]
async fn a_week_that_rolled_over_reloads_the_season_and_emits_the_new_one() {
    let mut harness = Harness::named("tick");
    let was = harness.season.lock().await.as_ref().expect("loaded").week;

    // First tick: the week has not moved, so nothing is reloaded.
    harness.tick().await;
    assert_eq!(harness.engine.reloads.get(), 0);

    // The NFL moves on, and the loader is ready with the new week's season.
    let mut next = harness
        .season
        .lock()
        .await
        .as_ref()
        .expect("loaded")
        .clone();
    next.week = was + 1;
    harness.engine.reloaded = Some(next);
    harness.engine.week.set(was + 1);

    // The ten-minute check is not due again yet, so the poller is still on the
    // old week — a tick is thirty seconds and the rollover is a weekly event.
    harness.tick().await;
    assert_eq!(harness.engine.reloads.get(), 0, "the check is rate limited");

    // Once it is due, the whole season is reloaded rather than live-refreshed:
    // every roster, matchup and projection belongs to a different week.
    harness.memory = SeasonPollMemory::new(20);
    let tick = harness.tick().await;
    assert_eq!(harness.engine.reloads.get(), 1);
    assert_eq!(
        harness.season.lock().await.as_ref().expect("loaded").week,
        was + 1
    );
    let view = tick.view.expect("the new week has to reach the screen");
    assert_eq!(view.week, was + 1);
}

/// A rollover check that cannot reach Sleeper leaves the week alone. Guessing
/// would be worse than showing the week we know we have data for.
#[tokio::test]
async fn a_failed_week_check_changes_nothing() {
    let mut harness = Harness::named("tick");
    let was = harness.season.lock().await.as_ref().expect("loaded").week;
    harness.engine.failing.set(true);
    harness.tick().await;
    assert_eq!(harness.engine.reloads.get(), 0);
    assert_eq!(
        harness.season.lock().await.as_ref().expect("loaded").week,
        was
    );
}
