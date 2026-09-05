//! What the season poller keeps current while the app is left open: the Trends
//! history across a week rollover, the player dictionary behind it, and how
//! much of the loaded league a tick has to copy to do any of it.

mod common;
mod poll_support;

use draft_assistant_lib::poll::{refresh_or_roll, SeasonPollMemory};
use draft_assistant_lib::season_history::HistoryStore;
use draft_assistant_lib::sleeper::PlayerMeta;
use poll_support::Harness;
use std::collections::HashMap;
use tokio::sync::Mutex;

/// Every player id the fixture league knows about.
///
/// A refresh now has to still know most of the league before it is allowed to
/// replace the loaded dictionary, so a test that wants one accepted has to
/// hand over a whole board rather than the row it cares about.
fn every_player() -> Vec<String> {
    common::fixture()
        .0
        .board
        .iter()
        .map(|p| p.player_id.clone())
        .collect()
}

/// A player dictionary covering `ids`, with an injury status on the ones named
/// in `injuries`. The shape Sleeper's dictionary endpoint answers with.
fn dictionary(ids: &[String], injuries: &[(&str, &str)]) -> HashMap<String, PlayerMeta> {
    ids.iter()
        .map(|id| {
            let status = injuries
                .iter()
                .find(|(who, _)| who == id)
                .map(|(_, status)| serde_json::Value::from(*status))
                .unwrap_or(serde_json::Value::Null);
            let meta = serde_json::from_value(serde_json::json!({ "injury_status": status }))
                .expect("a player row");
            (id.clone(), meta)
        })
        .collect()
}

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
    std::sync::Arc::make_mut(&mut next.rosters)[0].players =
        Some(vec!["q1".into(), "r1".into(), "w1".into(), "w2".into()]);
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
    harness.engine.players = Some(dictionary(&every_player(), &[("q1", "Questionable")]));

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

/// The bug: `is_usable` only asked whether the dictionary had anything in it,
/// so a truncated or half-parsed body — which still deserialises — was swapped
/// in and every player it had lost went with it. Names blanked and injury
/// tags cleared, from a response that was never complete.
#[tokio::test]
async fn a_dictionary_that_lost_most_of_the_league_is_refused() {
    let mut harness = Harness::named("partial-dictionary");
    let before = harness.tick().await.view.expect("the first view is sent");
    let name_of = |view: &draft_assistant_lib::season::SeasonView| {
        view.roster
            .iter()
            .find(|row| row.player_id == "r1")
            .map(|row| row.name.clone())
            .expect("the fixture rosters r1")
    };
    assert_eq!(name_of(&before), "Lead Back");

    // A body carrying one player out of nineteen, which is what a truncated
    // download looks like once serde has had a go at it.
    harness.engine.players = Some(dictionary(&["q1".to_string()], &[("q1", "Questionable")]));
    harness.memory = SeasonPollMemory::new(20);
    let after = harness.tick().await.view.expect("a view is still built");

    assert_eq!(
        name_of(&after),
        "Lead Back",
        "a partial dictionary was swapped in and took the rest of the board with it"
    );
    let flagged = after
        .matchup
        .as_ref()
        .expect("a matchup")
        .rows
        .iter()
        .any(|row| row.my_injury.is_some());
    assert!(!flagged, "a refused refresh must change nothing at all");
    assert!(
        after
            .data_health
            .warnings
            .iter()
            .any(|w| w.contains("came back incomplete")),
        "keeping the old dictionary silently is its own bug: {:?}",
        after.data_health.warnings
    );
}

/// The bug: Refresh asked only for the live slice, and it asks for it *by
/// week*. From Tuesday morning the button re-fetched the week that had just
/// finished, forever, and the only way to see the new one was to close the
/// league and open it again.
#[tokio::test]
async fn the_refresh_button_rolls_the_week_over() {
    let mut harness = Harness::named("refresh-rollover");
    let was = harness.season.lock().await.as_ref().expect("loaded").week;

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

    refresh_or_roll(
        &harness.engine,
        &harness.loaded,
        &harness.season,
        &harness.config,
    )
    .await
    .expect("the refresh must succeed");

    assert_eq!(
        harness.season.lock().await.as_ref().expect("loaded").week,
        was + 1,
        "Refresh kept watching the week that had already finished"
    );
    assert_eq!(harness.engine.reloads.get(), 1, "the full load never ran");
}

/// The bug: every tick built the whole view — two lineup solves always, and
/// on a rebuild tick the thousand-odd solves and the playoff simulation
/// behind them — and only then compared it against the last one and threw it
/// away. Between Tuesday and Saturday that is minutes of CPU an hour spent to
/// emit nothing at all.
#[tokio::test]
async fn a_tick_with_nothing_moving_builds_nothing() {
    let mut harness = Harness::named("quiet-tick");
    harness.tick().await.view.expect("the first view is sent");
    assert_eq!(harness.memory.builds(), 1);

    let quiet = harness.tick().await;
    assert!(quiet.view.is_none(), "an unchanged view was emitted");
    assert_eq!(
        harness.memory.builds(),
        1,
        "the solver ran again for a tick where nothing had moved"
    );
    assert!(
        quiet.health.is_some(),
        "the health badge still has to hear that the attempt was made"
    );

    // And scoring that actually moved is built again. A starter's own points
    // count as movement even when his team's total has not caught up, which
    // is what a mid-afternoon lineup swap looks like on the wire.
    harness.engine.matchups[0].players_points = Some(HashMap::from([("q1".to_string(), 41.5)]));
    harness.tick().await;
    assert_eq!(
        harness.memory.builds(),
        2,
        "a real change was mistaken for a quiet tick"
    );
}

/// The season half of the same copy: the tick used to deep-copy fifteen weeks
/// of matchups, a season of transactions and the whole Trends history out
/// from under the mutex every thirty seconds.
#[tokio::test]
async fn the_tick_input_shares_the_season_instead_of_copying_it() {
    let (loaded, season, config) = common::fixture();
    let rosters = season.rosters.clone();
    let schedule = season.schedule.clone();
    let transactions = season.transactions.clone();
    let history = season.history.clone();
    let loaded = Mutex::new(Some(loaded));
    let season = Mutex::new(Some(season));
    let config = Mutex::new(config);

    let inputs = draft_assistant_lib::state::season_inputs(&loaded, &season, &config)
        .await
        .expect("the fixture has a league and a season");

    for (shared, what) in [
        (
            std::sync::Arc::ptr_eq(&inputs.season().rosters, &rosters),
            "rosters",
        ),
        (
            std::sync::Arc::ptr_eq(&inputs.season().schedule, &schedule),
            "the schedule",
        ),
        (
            std::sync::Arc::ptr_eq(&inputs.season().transactions, &transactions),
            "the transactions",
        ),
        (
            std::sync::Arc::ptr_eq(&inputs.season().history, &history),
            "the Trends history",
        ),
    ] {
        assert!(shared, "the tick copied {what} rather than pointing at it");
    }
}
