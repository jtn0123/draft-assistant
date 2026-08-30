//! Engine loaders when Sleeper is unreachable.
//!
//! No real network traffic: every request is routed through a proxy address
//! on a closed local port, so each fetch fails instantly with a connection
//! error and the error-handling paths run exactly as they would in an outage.

use draft_assistant_lib::engine::Engine;
use draft_assistant_lib::sleeper::League;
use std::sync::Once;

static OFFLINE: Once = Once::new();

/// Route all HTTP through a dead local port. The listener is bound only to
/// reserve a port that nothing is listening on, then dropped so connections
/// to it are refused immediately.
fn offline_engine(label: &str) -> Engine {
    OFFLINE.call_once(|| {
        let port = std::net::TcpListener::bind("127.0.0.1:0")
            .and_then(|l| l.local_addr())
            .map(|a| a.port())
            .unwrap_or(9);
        let proxy = format!("http://127.0.0.1:{port}");
        std::env::set_var("HTTP_PROXY", &proxy);
        std::env::set_var("HTTPS_PROXY", &proxy);
    });
    let dir = std::env::temp_dir().join(format!(
        "draft-assistant-offline-{label}-{}-{}",
        std::process::id(),
        std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .unwrap()
            .as_nanos()
    ));
    Engine::new(dir)
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
        .load_any("mystery-id", true)
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
        rosters: Vec::new(),
        matchups: Vec::new(),
        schedule: Vec::new(),
        season_points: Default::default(),
        transactions: Vec::new(),
        scores: Vec::new(),
        last_season: Vec::new(),
        history: Default::default(),
        fetched_at: stamped,
        warnings: Vec::new(),
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
    std::fs::remove_dir_all(engine.data_dir).unwrap();
}
