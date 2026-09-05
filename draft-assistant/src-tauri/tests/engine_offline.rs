//! Engine loaders when Sleeper is unreachable.
//!
//! No real network traffic: this engine's client is built for a dead local
//! port, so each fetch fails instantly with a connection error and the
//! error-handling paths run exactly as they would in an outage. The client
//! carries that host itself — nothing here touches the environment, which is
//! shared with every other test thread in the binary.

use draft_assistant_lib::engine::Engine;
use draft_assistant_lib::season_engine::SeasonLoader;
use draft_assistant_lib::sleeper::{League, SleeperClient};

/// Port 1 is reserved and needs root to bind, so nothing is listening and
/// every connection to it is refused immediately.
const DEAD_HOST: &str = "http://127.0.0.1:1";

/// An engine whose every Sleeper URL goes to [`DEAD_HOST`]. `with_host` also
/// ignores `HTTP_PROXY`/`HTTPS_PROXY`, so no shell setting can route these
/// requests anywhere else — least of all out of the machine.
fn offline_engine(label: &str) -> Engine {
    let dir = std::env::temp_dir().join(format!(
        "draft-assistant-offline-{label}-{}-{}",
        std::process::id(),
        std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .unwrap()
            .as_nanos()
    ));
    Engine::with_client(dir, SleeperClient::with_host(DEAD_HOST))
}

/// The point of the whole file: these requests go to the dead port and
/// nowhere else. The error names the URL that was actually attempted, so it
/// is proof of the destination rather than of the failure alone.
#[tokio::test]
async fn every_request_goes_to_the_dead_port_and_not_to_sleeper() {
    let engine = offline_engine("destination");
    let err = engine
        .load_league("league-1", true)
        .await
        .err()
        .expect("expected an offline failure");
    assert!(err.contains("127.0.0.1:1"), "unexpected error: {err}");
    assert!(!err.contains("api.sleeper.app"), "unexpected error: {err}");
    std::fs::remove_dir_all(engine.data_dir).unwrap();
}

fn league_json() -> League {
    serde_json::from_str(
        r#"{
            "league_id": "league-1",
            "name": "Offline League",
            "season": "2025",
            "status": "in_season",
            "total_rosters": 10,
            "roster_positions": ["QB", "RB", "WR", "TE", "FLEX", "BN"],
            "scoring_settings": {"rec": 1.0},
            "draft_id": "draft-1"
        }"#,
    )
    .unwrap()
}

#[tokio::test]
async fn load_league_surfaces_the_fetch_error() {
    let engine = offline_engine("load-league");
    let err = engine
        .load_league("league-1", false)
        .await
        .err()
        .expect("expected an offline failure");
    assert!(err.contains("request failed"), "unexpected error: {err}");
    assert!(err.contains("league/league-1"), "unexpected error: {err}");
    std::fs::remove_dir_all(engine.data_dir).unwrap();
}

#[tokio::test]
async fn load_draft_only_surfaces_the_fetch_error() {
    let engine = offline_engine("load-draft");
    let err = engine
        .load_draft_only("draft-1", false)
        .await
        .err()
        .expect("expected an offline failure");
    assert!(err.contains("request failed"), "unexpected error: {err}");
    assert!(err.contains("draft/draft-1"), "unexpected error: {err}");
    std::fs::remove_dir_all(engine.data_dir).unwrap();
}

#[tokio::test]
async fn load_any_explains_both_failed_interpretations() {
    let engine = offline_engine("load-any");
    let err = engine
        .load_any("mystery-id", true, None)
        .await
        .err()
        .expect("expected an offline failure");
    assert!(err.contains("not a league ("), "unexpected error: {err}");
    assert!(err.contains("not a draft ("), "unexpected error: {err}");
    std::fs::remove_dir_all(engine.data_dir).unwrap();
}

#[tokio::test]
async fn load_season_fails_when_nfl_state_is_unreachable() {
    let engine = offline_engine("load-season");
    let err = engine
        .load_season(&league_json(), Some("user-1"), false)
        .await
        .err()
        .expect("expected an offline failure");
    assert!(err.contains("request failed"), "unexpected error: {err}");
    std::fs::remove_dir_all(engine.data_dir).unwrap();
}

#[tokio::test]
async fn refresh_live_reports_a_total_outage_instead_of_claiming_freshness() {
    use draft_assistant_lib::season_engine::LoadedSeason;

    let engine = offline_engine("refresh-live");
    let stamped = 1_000u64;
    let mut season = LoadedSeason {
        week: 2,
        season: 2025,
        rosters: std::sync::Arc::new(Vec::new()),
        matchups: std::sync::Arc::new(Vec::new()),
        schedule: std::sync::Arc::new(Vec::new()),
        season_points: std::sync::Arc::new(Default::default()),
        transactions: std::sync::Arc::new(Vec::new()),
        scores: std::sync::Arc::new(Vec::new()),
        last_season: std::sync::Arc::new(Vec::new()),
        history: std::sync::Arc::new(Default::default()),
        fetched_at: stamped,
        warnings: Vec::new(),
        sources: Default::default(),
    };

    let err = engine
        .refresh_live(&mut season, "league-1")
        .await
        .expect_err("every endpoint is unreachable, so the refresh must fail");
    for endpoint in ["matchups", "scores", "rosters"] {
        assert!(err.contains(endpoint), "{endpoint} missing from: {err}");
    }
    // The staleness clock must not move, or the health badge goes green on
    // data that never arrived.
    assert_eq!(season.fetched_at, stamped);
    // Each source keeps its own reason, so the badge can name what is broken.
    for status in [
        &season.sources.matchups,
        &season.sources.scores,
        &season.sources.rosters,
    ] {
        assert!(status.error.is_some(), "every source failed");
        assert_eq!(status.last_success_secs, 0, "none of them has ever worked");
    }
    std::fs::remove_dir_all(engine.data_dir).unwrap();
}
