//! What the season poller keeps current while the app is left open: the Trends
//! history across a week rollover, the player dictionary behind it, and how
//! much of the loaded league a tick has to copy to do any of it.

mod common;
mod poll_support;

use draft_assistant_lib::poll::SeasonPollMemory;
use draft_assistant_lib::season_history::HistoryStore;
use poll_support::Harness;
use std::collections::HashMap;
use tokio::sync::Mutex;

/// The rollover used to hand the poller a season built by `Engine::load_season`,
/// which leaves `history` empty because the Trends file is the command layer's
/// business. Only the user-driven load ever filled it in, so every Tuesday
/// morning the graph an app left running had been building all season emptied
/// itself, and the week that had just finished was never recorded at all.
#[tokio::test]
async fn a_week_rollover_keeps_the_trend_history_and_adds_to_it() {
    let mut harness = Harness::named("rollover-history");
    let was = harness.season.lock().await.as_ref().expect("loaded").week;

    // A reading already on file, taken while the old week was still running.
    let seeded = {
        let loaded = harness.loaded.lock().await;
        let season = harness.season.lock().await;
        harness
            .engine
            .history
            .record_history(
                loaded.as_ref().expect("loaded"),
                season.as_ref().expect("loaded"),
            )
            .await
    };
    assert_eq!(seeded.snapshots.len(), 1, "the fixture starts with one");
    let seeded_at = seeded.snapshots[0].taken_at;

    // The NFL moves on. The new week's season arrives with a roster change,
    // which is what a real waiver run between weeks looks like — and what
    // makes the next snapshot worth recording inside the quiet window.
    let mut next = harness
        .season
        .lock()
        .await
        .as_ref()
        .expect("loaded")
        .clone();
    next.week = was + 1;
    next.rosters[0].players = Some(vec!["q1".into(), "r1".into(), "w1".into(), "w2".into()]);
    harness.engine.reloaded = Some(next);
    harness.engine.week.set(was + 1);
    harness.memory = SeasonPollMemory::new(20);

    harness.tick().await;

    let history = harness
        .season
        .lock()
        .await
        .as_ref()
        .expect("loaded")
        .history
        .clone();
    assert_eq!(
        history.snapshots.len(),
        2,
        "the rollover must keep what was on file and add the new week"
    );
    assert_eq!(history.snapshots[0].taken_at, seeded_at);
    assert_eq!(history.snapshots[1].week, was + 1);
}

/// Injury statuses were read once, by the load that opened the league, and
/// never again: a starter ruled out on Saturday night was still projected and
/// still in the optimal lineup all Sunday, with no way to notice short of
/// reloading the league by hand.
#[tokio::test]
async fn a_changed_injury_status_reaches_the_next_view_without_a_reload() {
    let mut harness = Harness::named("injury-refresh");

    let before = harness.tick().await.view.expect("the first view is sent");
    let flag_of = |view: &draft_assistant_lib::season::SeasonView| {
        view.matchup
            .as_ref()
            .expect("the fixture has a matchup")
            .rows
            .iter()
            .find(|row| row.my_player_id.as_deref() == Some("q1"))
            .and_then(|row| row.my_injury.clone())
    };
    assert_eq!(flag_of(&before), None, "the fixture starts him healthy");

    // The dictionary now lists him Questionable. Nothing else changed, and
    // nobody reloaded the league.
    harness.engine.players = Some(HashMap::from([(
        "q1".to_string(),
        serde_json::from_value(serde_json::json!({"injury_status": "Questionable"}))
            .expect("a player row"),
    )]));

    // A poller whose half-hour clock has come round again. (That the clock is
    // half an hour, and that it is separate from the week check, is pinned in
    // `season_engine::week_watch`.)
    harness.memory = SeasonPollMemory::new(20);
    let after = harness
        .tick()
        .await
        .view
        .expect("a changed injury is a changed view");
    assert_eq!(flag_of(&after), Some("Q".to_string()));
    // The refresh came back with no projections at all, which is what a failed
    // projections fetch looks like. Rebuilding from that would zero every
    // number on the screen, so the ones already loaded have to survive it.
    assert!(
        (after.header.my_projected - before.header.my_projected).abs() < 1e-9,
        "an empty projection set blanked the projections"
    );
    assert!(
        harness.engine.refreshes.get() >= 2,
        "the poller has to ask for the dictionary again"
    );
}

/// The tick used to deep-copy the whole loaded league out from under its mutex
/// every thirty seconds — the board, its index, the player dictionary and the
/// weekly projections, megabytes of `Vec` and `HashMap` per tick. They are
/// shared now, so the copy is four pointer bumps.
#[tokio::test]
async fn the_tick_input_shares_the_board_instead_of_copying_it() {
    let (loaded, season, config) = common::fixture();
    let board = loaded.board.clone();
    let weekly = loaded.weekly_points.clone();
    let loaded = Mutex::new(Some(loaded));
    let season = Mutex::new(Some(season));
    let config = Mutex::new(config);

    let inputs = draft_assistant_lib::state::season_inputs(&loaded, &season, &config)
        .await
        .expect("the fixture has a league and a season");

    assert!(
        std::sync::Arc::ptr_eq(&inputs.league().board, &board),
        "the tick copied the board rather than pointing at it"
    );
    assert!(
        std::sync::Arc::ptr_eq(&inputs.league().weekly_points, &weekly),
        "the tick copied the weekly projections rather than pointing at them"
    );
}
