//! The two pollers, driven through the commands the screen calls on mount.
//!
//! Split out of `command_flows.rs` so that file stays inside the project's
//! line limit. These are the tests that need the event bus rather than just
//! the IPC return value, which is why they moved together.

use super::session;
use crate::routes::LEAGUE_ID;
use serde_json::{json, Value};
use std::sync::Arc;
use tauri::Listener;
use tokio::sync::Mutex;

#[test]
fn stopping_a_poll_that_never_started_is_not_an_error() {
    let s = session("stop-poll");
    s.ok("stop_polling", json!({}));
    s.ok("stop_season_polling", json!({}));
    s.finish();
}

/// Wait for `event` to be emitted, up to two seconds. Returns its payload.
fn await_event(app: &tauri::App<tauri::test::MockRuntime>, event: &str) -> Option<String> {
    let seen: Arc<Mutex<Option<String>>> = Arc::new(Mutex::new(None));
    let heard = seen.clone();
    app.listen_any(event.to_string(), move |event| {
        if let Ok(mut slot) = heard.try_lock() {
            *slot = Some(event.payload().to_string());
        }
    });
    for _ in 0..200 {
        if let Ok(slot) = seen.try_lock() {
            if slot.is_some() {
                return slot.clone();
            }
        }
        std::thread::sleep(std::time::Duration::from_millis(10));
    }
    None
}

#[test]
fn the_draft_poller_reports_its_health_on_every_tick() {
    let s = session("draft-poll");
    s.ok("add_league", json!({"leagueId": LEAGUE_ID, "force": true}));
    s.ok("start_polling", json!({"intervalSecs": 2}));

    let health = await_event(&s.app, "poll-health").expect("the first tick reports in");
    let health: Value = serde_json::from_str(&health).expect("the payload is JSON");
    // The stub answers, so the badge must say the feed is working rather than
    // sitting on whatever it said at load.
    assert_eq!(health["consecutive_failures"], 0, "{health}");
    assert!(health["last_success_at"].is_number(), "{health}");

    s.ok("stop_polling", json!({}));
    s.finish();
}

#[test]
fn the_season_poller_reports_its_health_too() {
    let s = session("season-poll");
    s.ok("add_league", json!({"leagueId": LEAGUE_ID, "force": true}));
    s.ok("load_season", json!({"force": true}));
    s.ok("start_season_polling", json!({"intervalSecs": 10}));

    let health = await_event(&s.app, "season-poll-health").expect("the first tick reports in");
    let health: Value = serde_json::from_str(&health).expect("the payload is JSON");
    // Health is emitted before any view, so a failing feed still says so.
    assert!(health.is_object(), "{health}");

    s.ok("stop_season_polling", json!({}));
    s.finish();
}

#[test]
fn a_poller_started_before_a_league_is_open_simply_waits_for_one() {
    // Starting the poll is not something that can go wrong: the screen calls
    // it on mount, which is often before a league has finished loading.
    let s = session("early-poll");
    s.ok("start_polling", json!({}));
    s.ok("start_season_polling", json!({}));
    s.ok("stop_polling", json!({}));
    s.ok("stop_season_polling", json!({}));
    s.finish();
}
