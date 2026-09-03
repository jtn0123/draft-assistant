//! Where a chat question gets its season summary from, and — the point of the
//! whole arrangement — what is locked while it does.
//!
//! Rebuilding a season view is seconds of arithmetic. If a question does it on
//! the runtime thread with `loaded`, `season` and `config` held, the 30-second
//! season poller and the 3-second draft poller both stop until the answer is
//! ready. These tests pin down that it does not.

mod common;

use draft_assistant_lib::season::build_season_view;
use draft_assistant_lib::state::{
    build_season_off_thread, season_inputs, season_view_for_chat, CachedSeasonView,
};
use std::sync::Arc;
use tokio::sync::Mutex;

type Shared = (
    Arc<Mutex<Option<draft_assistant_lib::engine::LoadedLeague>>>,
    Arc<Mutex<Option<draft_assistant_lib::season_engine::LoadedSeason>>>,
    Arc<Mutex<draft_assistant_lib::engine::AppConfig>>,
);

fn shared() -> Shared {
    let (loaded, season, config) = common::fixture();
    (
        Arc::new(Mutex::new(Some(loaded))),
        Arc::new(Mutex::new(Some(season))),
        Arc::new(Mutex::new(config)),
    )
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn the_build_runs_with_every_guard_already_dropped() {
    let (loaded, season, config) = shared();

    // Phase one: copy the inputs. This is the only part that locks anything.
    let inputs = season_inputs(&loaded, &season, &config).await.unwrap();

    // The moment the copy is done all three are free again — so whatever the
    // build does next, it cannot be doing it with a guard in hand. Standing in
    // for a poll tick, take all three and hold them.
    let held_loaded = loaded.try_lock().expect("loaded is free after the copy");
    let held_season = season.try_lock().expect("season is free after the copy");
    let held_config = config.try_lock().expect("config is free after the copy");

    // Phase two: the expensive half, while the "poller" still holds the locks.
    // It finishes, which it could not do if it needed any of them.
    let view = build_season_off_thread(inputs).await.unwrap();
    assert_eq!(view.league.league_id, "league-1");
    assert_eq!(view.week, 2);

    drop((held_loaded, held_season, held_config));
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn a_question_reuses_the_view_the_season_screen_already_built() {
    let (loaded, season, config) = shared();
    let last = Arc::new(Mutex::new(None));

    // Nothing cached yet: chat builds one, and keeps it.
    let built = season_view_for_chat(&loaded, &season, &config, &last)
        .await
        .unwrap();
    assert!(last.lock().await.is_some(), "the build is remembered");

    // The second question gets the very same view back — not an equal one, the
    // same allocation — so no second build happened.
    let again = season_view_for_chat(&loaded, &season, &config, &last)
        .await
        .unwrap();
    assert!(
        Arc::ptr_eq(&built, &again),
        "the cached view must be reused"
    );
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn a_cached_view_from_another_league_is_not_reused() {
    let (loaded, season, config) = shared();
    let (other_loaded, other_season, other_config) = common::fixture();
    let mut stale = build_season_view(
        &other_loaded,
        &other_season,
        other_config.my_user_id.as_deref(),
    );
    stale.league.league_id = "some-other-league".to_string();
    let last = Arc::new(Mutex::new(Some(CachedSeasonView::new(Arc::new(stale)))));

    let view = season_view_for_chat(&loaded, &season, &config, &last)
        .await
        .unwrap();
    assert_eq!(
        view.league.league_id, "league-1",
        "a view left over from the league the user switched away from \
         must not answer the question"
    );
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn nothing_loaded_is_reported_rather_than_guessed_at() {
    let (loaded, season, config) = shared();
    let last = Arc::new(Mutex::new(None));

    *season.lock().await = None;
    let error = season_view_for_chat(&loaded, &season, &config, &last)
        .await
        .unwrap_err();
    assert_eq!(error, "season data not loaded");

    *loaded.lock().await = None;
    let error = season_view_for_chat(&loaded, &season, &config, &last)
        .await
        .unwrap_err();
    assert_eq!(error, "no league loaded");
}
