//! Optional member lookups must not hold successful draft picks behind retries.
use super::*;
use axum::{extract::State, routing::get, Json, Router};
use std::sync::atomic::{AtomicUsize, Ordering};
use std::time::Duration;

async fn run_tick(hang: bool) -> (TickFetch, usize) {
    let calls = Arc::new(AtomicUsize::new(0));
    let app = Router::new()
        .route(
            "/v1/draft/d/picks",
            get(|| async { Json(serde_json::json!([])) }),
        )
        .route(
            "/v1/draft/d",
            get(|| async {
                Json(
                    serde_json::json!({"draft_id":"d", "status":"drafting", "type":"snake",
                "settings":{"teams":12,"rounds":15}}),
                )
            }),
        )
        .route(
            "/v1/draft/d/traded_picks",
            get(|| async { Json(serde_json::json!([])) }),
        )
        .route(
            "/v1/league/l/users",
            get(move |State(calls): State<Arc<AtomicUsize>>| async move {
                calls.fetch_add(1, Ordering::SeqCst);
                if hang {
                    std::future::pending::<()>().await;
                }
                axum::http::StatusCode::SERVICE_UNAVAILABLE
            }),
        )
        .with_state(calls.clone());
    let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
    let host = format!("http://{}", listener.local_addr().unwrap());
    let server = tokio::spawn(async move {
        axum::serve(listener, app).await.unwrap();
    });
    let (state, dir) = AppState::scratch("member-tick");
    let engine = Engine::with_client(dir.clone(), crate::sleeper::SleeperClient::with_host(host));
    let result = tokio::time::timeout(
        Duration::from_secs(5),
        fetch_tick(&engine, &state.yahoo, "d", &HashMap::new(), Some("l")),
    )
    .await;
    server.abort();
    let _ = std::fs::remove_dir_all(dir);
    (
        result.expect("successful picks waited for optional member retries"),
        calls.load(Ordering::SeqCst),
    )
}

#[tokio::test]
async fn a_failing_member_lookup_is_not_retried_inside_the_tick() {
    let (fetched, calls) = run_tick(false).await;
    assert!(fetched.picks.is_ok());
    assert!(matches!(fetched.users, Some(Err(_))));
    assert_eq!(calls, 1, "optional members used the full retry policy");
}

#[tokio::test]
async fn a_hanging_member_lookup_does_not_hold_picks_for_full_timeout() {
    let (fetched, calls) = run_tick(true).await;
    assert!(fetched.picks.is_ok());
    assert!(matches!(fetched.users, Some(Err(_))));
    assert_eq!(calls, 1);
}
