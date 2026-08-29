//! The engine's fetch, cache and fallback matrix, driven through a stub
//! Sleeper: what is requested when, what a failure degrades to, and what the
//! warnings say. This is the code that decides what the board is built from.

mod support;

use draft_assistant_lib::engine::Engine;
use draft_assistant_lib::sleeper::SleeperClient;
use support::{Fixture, Reply, StubSleeper, DRAFT_ID, LEAGUE_ID, MOCK_DRAFT_ID};

fn engine_for(stub: &StubSleeper, label: &str) -> Engine {
    Engine {
        client: SleeperClient::with_base_url(&stub.base),
        data_dir: support::scratch_dir(label),
    }
}

/// Rewrite a cache envelope's timestamp so it reads as expired.
fn age_cache(engine: &Engine, name: &str) {
    let path = engine.data_dir.join(name);
    let mut value: serde_json::Value =
        serde_json::from_str(&std::fs::read_to_string(&path).unwrap()).unwrap();
    value["fetched_at"] = serde_json::json!(1);
    std::fs::write(&path, value.to_string()).unwrap();
}

#[tokio::test]
async fn a_cold_load_fetches_everything_and_a_warm_load_fetches_nothing() {
    let stub = StubSleeper::start();
    let fixture = Fixture::load();
    fixture.install(&stub);
    let engine = engine_for(&stub, "cold-warm");

    let loaded = engine.load_league(LEAGUE_ID, false).await.unwrap();
    assert_eq!(loaded.league.name, "Mixed lineup fixture");
    assert_eq!(loaded.board.len(), 6);
    assert_eq!(loaded.user_names.len(), 2);
    assert_eq!(stub.hits("/v1/players/nfl"), 1);
    assert_eq!(stub.hits("/projections/nfl/2026"), 1);
    assert_eq!(stub.hits("/projections/nfl/2026/1"), 1);
    assert!(
        loaded
            .warnings
            .iter()
            .any(|w| w.contains("board unusually small")),
        "{:?}",
        loaded.warnings
    );
    assert!(loaded.players_fetched_at > 0);

    // Every cache is fresh: the second load asks the server for nothing but
    // the league, draft, users and picks.
    stub.reset_hits();
    let again = engine.load_league(LEAGUE_ID, false).await.unwrap();
    assert_eq!(stub.hits("/v1/players/nfl"), 0);
    assert_eq!(stub.hits("/projections/nfl/2026"), 0);
    assert_eq!(stub.hits("/projections/nfl/2026/1"), 0);
    assert_eq!(again.players_fetched_at, loaded.players_fetched_at);

    // Force bypasses every cache.
    stub.reset_hits();
    engine.load_league(LEAGUE_ID, true).await.unwrap();
    assert_eq!(stub.hits("/v1/players/nfl"), 1);
    assert_eq!(stub.hits("/projections/nfl/2026/18"), 1);
}

#[tokio::test]
async fn an_expired_cache_is_refetched() {
    let stub = StubSleeper::start();
    let fixture = Fixture::load();
    fixture.install(&stub);
    let engine = engine_for(&stub, "expired");
    engine.load_league(LEAGUE_ID, false).await.unwrap();
    age_cache(&engine, "players.json");
    stub.reset_hits();
    engine.load_league(LEAGUE_ID, false).await.unwrap();
    assert_eq!(stub.hits("/v1/players/nfl"), 1);
    assert_eq!(stub.hits("/projections/nfl/2026"), 0, "still fresh");
}

#[tokio::test]
async fn a_failed_refresh_falls_back_to_the_stale_cache_with_a_dated_warning() {
    let stub = StubSleeper::start();
    let fixture = Fixture::load();
    fixture.install(&stub);
    let engine = engine_for(&stub, "stale-fallback");
    engine.load_league(LEAGUE_ID, false).await.unwrap();
    age_cache(&engine, "players.json");
    age_cache(&engine, "projections_2026.json");
    age_cache(&engine, "weekly_2026.json");
    stub.set("/v1/players/nfl", Reply::Status(500));
    stub.set("/projections/nfl/2026", Reply::Status(503));
    for week in 1..=18 {
        stub.set(&format!("/projections/nfl/2026/{week}"), Reply::Status(500));
    }

    let loaded = engine.load_league(LEAGUE_ID, false).await.unwrap();
    let warnings = loaded.warnings.join(" | ");
    assert!(
        warnings.contains("players refresh failed; using cache aged"),
        "{warnings}"
    );
    assert!(
        warnings.contains("projections refresh failed; using cache aged"),
        "{warnings}"
    );
    assert!(
        warnings.contains("weekly projections refresh failed; using cache aged"),
        "{warnings}"
    );
    assert!(warnings.contains("HTTP 500"), "{warnings}");
    assert_eq!(loaded.board.len(), 6, "the stale data still builds a board");
    assert_eq!(loaded.players_fetched_at, 1, "the age stamp is the cache's");
}

#[tokio::test]
async fn a_failed_fetch_with_no_cache_is_an_error() {
    let stub = StubSleeper::start();
    let fixture = Fixture::load();
    fixture.install(&stub);
    stub.set("/v1/players/nfl", Reply::Status(500));
    let engine = engine_for(&stub, "no-cache");
    let err = engine.load_league(LEAGUE_ID, false).await.err().unwrap();
    assert!(err.contains("HTTP 500"), "{err}");
    assert!(err.contains("/v1/players/nfl"), "{err}");

    // Same for season projections, once players are available.
    stub.json("/v1/players/nfl", &serde_json::json!({}));
    stub.set("/projections/nfl/2026", Reply::Status(502));
    let err = engine.load_league(LEAGUE_ID, false).await.err().unwrap();
    assert!(err.contains("HTTP 502"), "{err}");
}

#[tokio::test]
async fn missing_weeks_are_a_warning_but_all_weeks_missing_is_an_error() {
    let stub = StubSleeper::start();
    let fixture = Fixture::load();
    fixture.install(&stub);
    stub.set("/projections/nfl/2026/3", Reply::Status(500));
    stub.set("/projections/nfl/2026/5", Reply::Status(500));
    let engine = engine_for(&stub, "weeks");
    let loaded = engine.load_league(LEAGUE_ID, false).await.unwrap();
    assert!(
        loaded
            .warnings
            .iter()
            .any(|w| w == "weekly projections unavailable for weeks 3, 5"),
        "{:?}",
        loaded.warnings
    );

    for week in 1..=18 {
        stub.set(&format!("/projections/nfl/2026/{week}"), Reply::Status(500));
    }
    let fresh = engine_for(&stub, "weeks-none");
    let err = fresh.load_league(LEAGUE_ID, false).await.err().unwrap();
    assert_eq!(err, "all weekly projection requests failed");
}

#[tokio::test]
async fn a_cache_that_cannot_be_written_is_a_warning_not_a_failure() {
    let stub = StubSleeper::start();
    let fixture = Fixture::load();
    fixture.install(&stub);
    let engine = engine_for(&stub, "unwritable");
    std::fs::remove_dir_all(&engine.data_dir).unwrap();
    let loaded = engine.load_league(LEAGUE_ID, false).await.unwrap();
    let warnings = loaded.warnings.join(" | ");
    assert!(
        warnings.contains("players.json could not be cached"),
        "{warnings}"
    );
    assert!(warnings.contains("will refetch"), "{warnings}");
    assert_eq!(loaded.board.len(), 6);
}

#[tokio::test]
async fn a_failed_initial_picks_fetch_is_recorded_as_poll_health_and_a_warning() {
    let stub = StubSleeper::start();
    let fixture = Fixture::load();
    fixture.install(&stub);
    stub.set(&format!("/v1/draft/{DRAFT_ID}/picks"), Reply::Status(500));
    let engine = engine_for(&stub, "picks-fail");
    let loaded = engine.load_league(LEAGUE_ID, false).await.unwrap();
    assert_eq!(loaded.poll_consecutive_failures, 1);
    assert!(loaded.poll_last_success_at.is_none());
    assert!(loaded
        .poll_last_error
        .as_deref()
        .unwrap()
        .contains("HTTP 500"));
    assert!(loaded
        .warnings
        .iter()
        .any(|w| w.starts_with("initial picks refresh failed")));
}

#[tokio::test]
async fn a_mock_draft_synthesizes_its_league_from_the_draft_settings() {
    let stub = StubSleeper::start();
    let fixture = Fixture::load();
    fixture.install(&stub);
    let engine = engine_for(&stub, "mock");
    let loaded = engine.load_draft_only(MOCK_DRAFT_ID, false).await.unwrap();
    assert_eq!(
        loaded.league.roster_positions,
        vec!["QB", "RB", "WR", "TE", "FLEX", "K", "DEF", "BN"]
    );
    assert_eq!(loaded.league.scoring_settings.get("rec"), Some(&0.5));
    assert_eq!(loaded.league.name, "Mock");
    assert!(loaded
        .warnings
        .iter()
        .any(|w| w.starts_with("mock draft: league settings synthesized")));
}

#[tokio::test]
async fn load_any_tries_a_league_first_then_a_bare_draft_and_reports_both_failures() {
    let stub = StubSleeper::start();
    let fixture = Fixture::load();
    fixture.install(&stub);
    let engine = engine_for(&stub, "load-any");
    assert_eq!(
        engine.load_any(LEAGUE_ID, false).await.unwrap().league.name,
        "Mixed lineup fixture"
    );
    assert_eq!(
        engine
            .load_any(MOCK_DRAFT_ID, false)
            .await
            .unwrap()
            .league
            .name,
        "Mock"
    );
    let err = engine.load_any("nothing-here", false).await.err().unwrap();
    assert!(err.starts_with("not a league ("), "{err}");
    assert!(err.contains("; not a draft ("), "{err}");
    assert!(err.contains("HTTP 404"), "{err}");
}

#[tokio::test]
async fn a_league_without_a_draft_and_a_null_league_are_clear_errors() {
    let stub = StubSleeper::start();
    let fixture = Fixture::load();
    fixture.install(&stub);
    let engine = engine_for(&stub, "no-draft");
    let mut league = fixture.league.clone();
    league.draft_id = None;
    stub.json(&format!("/v1/league/{LEAGUE_ID}"), &league);
    assert_eq!(
        engine.load_league(LEAGUE_ID, false).await.err().unwrap(),
        "league has no draft"
    );
    stub.set(
        &format!("/v1/league/{LEAGUE_ID}"),
        Reply::Json("null".into()),
    );
    let err = engine.load_league(LEAGUE_ID, false).await.err().unwrap();
    assert!(err.contains("not found (Sleeper returned null)"), "{err}");
}
