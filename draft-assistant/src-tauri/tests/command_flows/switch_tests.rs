//! What each command does with an answer that arrives after the user has
//! moved on, and with an answer that arrives spoiled.
//!
//! Every one of these used to end with one league's data written into
//! another's — its picks on the board, its manual-pick file, its keepers on
//! disk — or with a good board replaced by an empty or unusable one. Reaching
//! that code means making the switch happen while a request is genuinely in
//! flight, which is what the gates in `routes.rs` are for: the stub holds the
//! answer back, a second thread switches leagues under the command, and only
//! then is the answer let through.

use super::session;
use crate::routes::{
    Gate, DRAFT_ID, DRAFT_IS_BROKEN, LEAGUE_BROKEN, LEAGUE_ID, LEAGUE_LIVE, LEAGUE_REBUILD,
    LEAGUE_SWITCH, LEAGUE_TICK, LEAGUE_VANISH, LIVE_MATCHUPS, PICKS_VANISHED, REBUILD_PICKS,
    SWITCH_PICKS, TICK_PICKS,
};
use draft_assistant_lib::engine::LoadedLeague;
use draft_assistant_lib::state::AppState;
use serde_json::json;
use std::sync::atomic::Ordering;
use std::sync::Arc;
use std::time::Duration;
use tauri::Manager;
use tokio::sync::Mutex;

/// Wait for `gate` to take one more request than `before`, run `switch`, and
/// then let the answer through.
///
/// On its own thread, because the command under test is holding the test's
/// thread inside the IPC call while its request sits in the gate.
fn switch_while_in_flight(
    gate: &'static Gate,
    before: usize,
    switch: impl FnOnce() + Send + 'static,
) -> std::thread::JoinHandle<()> {
    std::thread::spawn(move || {
        for _ in 0..2000 {
            if gate.served() > before {
                break;
            }
            std::thread::sleep(Duration::from_millis(5));
        }
        switch();
        gate.release();
    })
}

/// The loaded league, as the poller and the commands share it.
fn loaded_of(s: &super::Session) -> Arc<Mutex<Option<LoadedLeague>>> {
    s.app.state::<AppState>().loaded.clone()
}

/// Stand a different league up in place of the loaded one: a new draft, an
/// empty board and no keepers, which is what a real switch leaves behind.
fn become_league(loaded: &Mutex<Option<LoadedLeague>>, league_id: &str, draft_id: &str) {
    let mut guard = tauri::async_runtime::block_on(loaded.lock());
    let loaded = guard.as_mut().expect("a league is loaded");
    loaded.league.league_id = league_id.to_string();
    loaded.draft.draft_id = draft_id.to_string();
    loaded.api_picks.clear();
    loaded.keeper_pick_nos.clear();
}

fn keepers_file(s: &super::Session, draft_id: &str) -> std::path::PathBuf {
    s.data_dir.join(format!("keepers_{draft_id}.json"))
}

#[test]
fn a_pick_refresh_that_lands_after_a_league_switch_is_thrown_away() {
    let s = session("switch-picks");
    s.ok(
        "add_league",
        json!({"leagueId": LEAGUE_SWITCH, "force": true}),
    );

    let loaded = loaded_of(&s);
    let before = SWITCH_PICKS.served();
    SWITCH_PICKS.hold();
    let switched = loaded.clone();
    let switcher = switch_while_in_flight(&SWITCH_PICKS, before, move || {
        become_league(&switched, LEAGUE_ID, DRAFT_ID);
    });

    let error = s.err("refresh_picks", json!({}));
    switcher.join().expect("the switching thread finished");
    assert!(error.contains("league changed"), "{error}");

    // The old league's picks must not be on the new league's board, and its
    // keepers must not have been written to the new league's file: that one is
    // permanent, and every later launch would read it back.
    let guard = tauri::async_runtime::block_on(loaded.lock());
    let now = guard.as_ref().expect("a league is loaded");
    assert!(now.api_picks.is_empty(), "the old league's picks were kept");
    assert!(now.keeper_pick_nos.is_empty(), "keepers crossed over");
    drop(guard);
    assert!(
        !keepers_file(&s, DRAFT_ID).exists(),
        "the old league's keepers were written to the new league's file"
    );
    s.finish();
}

#[test]
fn a_poll_tick_that_lands_after_a_league_switch_is_thrown_away() {
    let s = session("switch-tick");
    s.ok(
        "add_league",
        json!({"leagueId": LEAGUE_TICK, "force": true}),
    );

    let loaded = loaded_of(&s);
    let before = TICK_PICKS.served();
    TICK_PICKS.hold();
    let watched = loaded.clone();
    let switcher = switch_while_in_flight(&TICK_PICKS, before, move || {
        become_league(&watched, LEAGUE_ID, DRAFT_ID);
    });
    s.ok("start_polling", json!({"intervalSecs": 60}));
    switcher.join().expect("the switching thread finished");

    // The answer was let through the moment the switch landed; give the tick
    // its chance to apply it before deciding that it did not.
    std::thread::sleep(Duration::from_millis(500));
    s.ok("stop_polling", json!({}));

    let guard = tauri::async_runtime::block_on(loaded.lock());
    let now = guard.as_ref().expect("a league is loaded");
    assert!(
        now.api_picks.is_empty(),
        "the tick wrote the old league's picks into the new one"
    );
    assert!(now.keeper_pick_nos.is_empty(), "keepers crossed over");
    drop(guard);
    assert!(
        !keepers_file(&s, DRAFT_ID).exists(),
        "the tick wrote the old league's keepers to the new league's file"
    );
    s.finish();
}

#[test]
fn a_rebuild_that_lands_after_a_league_switch_does_not_reinstate_it() {
    let s = session("switch-rebuild");
    s.ok(
        "add_league",
        json!({"leagueId": LEAGUE_REBUILD, "force": true}),
    );

    let loaded = loaded_of(&s);
    let config = s.app.state::<AppState>().config.clone();
    let before = REBUILD_PICKS.served();
    REBUILD_PICKS.hold();
    let switcher = switch_while_in_flight(&REBUILD_PICKS, before, move || {
        become_league(&loaded, LEAGUE_ID, DRAFT_ID);
        tauri::async_runtime::block_on(config.lock()).active_league_id =
            Some(LEAGUE_ID.to_string());
    });

    let error = s.err("refresh_data", json!({}));
    switcher.join().expect("the switching thread finished");
    assert!(error.contains("league changed"), "{error}");

    // The league the user chose is still the one on screen.
    let view = s.ok("get_state", json!({}));
    assert_eq!(view["league"]["league_id"], LEAGUE_ID);
    s.finish();
}

#[test]
fn a_live_refresh_that_lands_after_a_league_switch_is_thrown_away() {
    let s = session("switch-live");
    s.ok(
        "add_league",
        json!({"leagueId": LEAGUE_LIVE, "force": true}),
    );
    s.ok("load_season", json!({"force": true}));

    let loaded = loaded_of(&s);
    let before = LIVE_MATCHUPS.served();
    LIVE_MATCHUPS.hold();
    let switcher = switch_while_in_flight(&LIVE_MATCHUPS, before, move || {
        become_league(&loaded, LEAGUE_ID, DRAFT_ID);
    });

    let error = s.err("refresh_season", json!({}));
    switcher.join().expect("the switching thread finished");
    assert!(
        error.contains("league changed"),
        "one league's live scoring was folded into another's season: {error}"
    );
    s.finish();
}

#[test]
fn a_pick_list_that_vanishes_mid_draft_does_not_wipe_the_board() {
    let s = session("vanishing-picks");
    let view = s.ok(
        "add_league",
        json!({"leagueId": LEAGUE_VANISH, "force": true}),
    );
    assert_eq!(
        view["recent_picks"].as_array().expect("picks").len(),
        1,
        "the fixture draft has a pick in it to lose"
    );

    // From here Sleeper answers /picks with null, which parses as no picks at
    // all. Mid-draft that is a lost response, not a cleared board.
    PICKS_VANISHED.store(true, Ordering::SeqCst);
    // A pull that pulled nothing is not a successful pull. This used to
    // answer Ok with the unchanged view, so the toast said "picks re-pulled —
    // 1 in" over an answer that had none.
    let error = s.err("refresh_picks", json!({}));
    assert!(
        error.contains("empty") && error.contains("already on the board"),
        "the toast has to say what happened: {error}"
    );

    let kept = s.ok("get_state", json!({}));
    assert_eq!(
        kept["recent_picks"].as_array().expect("picks").len(),
        1,
        "an empty answer wiped the picks off the board"
    );
    let health = &kept["data_health"];
    assert_eq!(health["poll_consecutive_failures"], 1);
    let reported = health["poll_last_error"].as_str().unwrap_or_default();
    assert!(
        reported.contains("empty"),
        "the reason must be said: {health}"
    );
    s.finish();
}

#[test]
fn a_draft_that_comes_back_with_no_teams_is_not_adopted() {
    let s = session("broken-draft");
    let view = s.ok(
        "add_league",
        json!({"leagueId": LEAGUE_BROKEN, "force": true}),
    );
    assert_eq!(view["draft"]["teams"], 2);

    // A draft being set up reports zero teams and zero rounds. Every board
    // calculation divides by them.
    DRAFT_IS_BROKEN.store(true, Ordering::SeqCst);
    let refreshed = s.ok("refresh_picks", json!({}));
    assert_eq!(
        refreshed["draft"]["teams"], 2,
        "a draft that cannot be laid out replaced one that could"
    );
    assert_eq!(refreshed["draft"]["rounds"], 3);
    let reported = refreshed["data_health"]["poll_last_error"]
        .as_str()
        .unwrap_or_default();
    assert!(reported.contains("teams"), "the reason must be said");
    s.finish();
}

#[test]
fn a_league_that_cannot_be_saved_is_not_left_half_added() {
    let s = session("unsaveable-config");
    // A directory standing where the config file itself belongs, so the
    // rename that puts it in place fails the way a full or read-only disk
    // would. (The temp file it is renamed from has a unique name per writer
    // now, so there is no fixed temp path to block instead.)
    std::fs::create_dir_all(s.data_dir.join("config.json")).expect("the blocker is in place");

    let error = s.err("add_league", json!({"leagueId": LEAGUE_ID, "force": true}));
    assert!(error.contains("could not save"), "{error}");

    // Nothing was half-committed: the picker does not show a league the next
    // launch would have no record of.
    let config = s.ok("get_config", json!({}));
    assert_eq!(config["leagues"], json!([]));
    assert!(config["active_league_id"].is_null(), "{config}");
    s.finish();
}
