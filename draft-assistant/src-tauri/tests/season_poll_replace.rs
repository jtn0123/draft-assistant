//! The season poller when the season it is watching is replaced under it:
//! by a command that reloads it, and by a week rollover that fails.

mod common;
mod poll_support;

use draft_assistant_lib::poll::{refresh_or_roll, SeasonPollMemory};
use draft_assistant_lib::season_engine::week_watch::ROLLOVER_FAILED;
use poll_support::Harness;

/// The bug: a command replacing `state.season` (a forced reload, a league
/// switch) moved nothing on the scoreboard, so the poller kept its held
/// analysis and its scoreboard signature and re-emitted the old season's
/// standings and waivers for up to twenty ticks over the new one.
#[tokio::test]
async fn a_season_replaced_by_a_command_is_rebuilt_on_the_next_tick() {
    let mut harness = Harness::named("replaced");
    harness.tick().await.view.expect("the first view is sent");
    assert_eq!(harness.memory.builds(), 1);

    // Nothing moved: no build, no emit. This is the quiet-tick rule.
    assert!(harness.tick().await.view.is_none());
    assert_eq!(harness.memory.builds(), 1);

    // A command puts a new season in place. Same scores, same scoreboard,
    // but everything the analysis was built from may differ.
    let mut next = harness
        .season
        .lock()
        .await
        .as_ref()
        .expect("loaded")
        .clone();
    next.restamp();
    *harness.season.lock().await = Some(next);

    let tick = harness.tick().await;
    assert_eq!(
        harness.memory.builds(),
        2,
        "the poller kept serving the analysis of a season that no longer exists"
    );
    assert!(
        tick.view.is_some(),
        "the rebuilt view has to reach the screen"
    );
    // And the tick after that is quiet again: one invalidation per replace.
    assert!(harness.tick().await.view.is_none());
    assert_eq!(harness.memory.builds(), 2);
}

/// The bug: a rollover whose season load failed returned an empty tick, so
/// from Tuesday morning the screen scored the finished week with nothing on
/// the badge or in the log saying the new one had been tried.
#[tokio::test]
async fn a_failed_rollover_puts_a_warning_on_the_badge_and_keeps_the_old_week_live() {
    let mut harness = Harness::named("rollover-failed");
    let was = harness.season.lock().await.as_ref().expect("loaded").week;
    harness.engine.week.set(was + 1);
    harness.engine.reloaded = None;

    let tick = harness.tick().await;
    assert_eq!(
        harness.engine.reloads.get(),
        1,
        "the rollover was attempted"
    );
    assert_eq!(
        harness.season.lock().await.as_ref().expect("loaded").week,
        was,
        "the week we have data for stays on screen"
    );
    let view = tick
        .view
        .expect("the old week is still refreshed and the warning has to be seen");
    assert!(
        view.data_health
            .warnings
            .iter()
            .any(|w| w.starts_with(ROLLOVER_FAILED)),
        "{:?}",
        view.data_health.warnings
    );
    assert_eq!(
        tick.health.expect("health").consecutive_failures,
        0,
        "the live feed itself is fine; the rollover is a warning, not a failed tick"
    );
}

/// The same failure through the Refresh button.
#[tokio::test]
async fn a_refresh_whose_rollover_fails_says_so_and_still_refreshes_the_old_week() {
    let harness = Harness::named("refresh-rollover-failed");
    let was = harness.season.lock().await.as_ref().expect("loaded").week;
    harness.engine.week.set(was + 1);

    refresh_or_roll(
        &harness.engine,
        &harness.loaded,
        &harness.season,
        &harness.config,
    )
    .await
    .expect("the old week's live slice still refreshes");

    let season = harness.season.lock().await;
    let season = season.as_ref().expect("loaded");
    assert_eq!(season.week, was);
    assert!(
        season
            .warnings
            .iter()
            .any(|w| w.starts_with(ROLLOVER_FAILED)),
        "{:?}",
        season.warnings
    );
    assert_eq!(harness.engine.reloads.get(), 1);
}

/// A view built after a rollover warning, and a later successful rollover,
/// carries the new week's own warnings and not the stale complaint.
#[tokio::test]
async fn a_rollover_that_later_succeeds_takes_the_warning_down() {
    let mut harness = Harness::named("rollover-recovers");
    let was = harness.season.lock().await.as_ref().expect("loaded").week;
    harness.engine.week.set(was + 1);
    harness.tick().await;
    assert!(harness
        .season
        .lock()
        .await
        .as_ref()
        .expect("loaded")
        .warnings
        .iter()
        .any(|w| w.starts_with(ROLLOVER_FAILED)));

    let mut next = harness
        .season
        .lock()
        .await
        .as_ref()
        .expect("loaded")
        .clone();
    next.week = was + 1;
    next.warnings.clear();
    harness.engine.reloaded = Some(next);
    harness.memory = SeasonPollMemory::new(20);
    let tick = harness.tick().await;
    let view = tick.view.expect("the new week reaches the screen");
    assert_eq!(view.week, was + 1);
    assert!(
        !view
            .data_health
            .warnings
            .iter()
            .any(|w| w.starts_with(ROLLOVER_FAILED)),
        "{:?}",
        view.data_health.warnings
    );
}
