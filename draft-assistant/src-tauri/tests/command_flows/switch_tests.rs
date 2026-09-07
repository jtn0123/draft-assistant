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
    Gate, DRAFT_ID, DRAFT_IS_BROKEN, HOLE_FULL, HOLE_LAST_UNDONE, HOLE_MISSING_ONE, HOLE_PICKS,
    LEAGUE_BROKEN, LEAGUE_HOLE, LEAGUE_ID, LEAGUE_LIVE, LEAGUE_REBUILD, LEAGUE_SWITCH, LEAGUE_TICK,
    LEAGUE_VANISH, LIVE_MATCHUPS, PICKS_VANISHED, REBUILD_PICKS, SWITCH_PICKS, TICK_PICKS,
};
use draft_assistant_lib::engine::LoadedLeague;
use draft_assistant_lib::state::AppState;
use serde_json::json;
use std::sync::atomic::Ordering;
use std::sync::Arc;
use std::time::Duration;
use tauri::Manager;
use tokio::sync::Mutex;

/// Wait until a request is actually sitting in `gate`, run `switch`, and then
/// let the answer through. Answers whether a request really was in flight.
///
/// On its own thread, because the command under test is holding the test's
/// thread inside the IPC call while its request sits in the gate.
///
/// The answer matters. This used to count requests *served* and then switch
/// after ten seconds regardless, with nothing said either way: a request that
/// came and went before the gate was held counted, and one that never arrived
/// counted for nothing but was not complained about either. Both left the
/// switch happening with nothing in flight and the discard path under test
/// never entered, with all four tests green. `assert_raced` below is what
/// every caller closes with.
fn switch_while_in_flight(
    gate: &'static Gate,
    switch: impl FnOnce() + Send + 'static,
) -> std::thread::JoinHandle<bool> {
    std::thread::spawn(move || {
        let mut arrived = false;
        for _ in 0..2000 {
            if gate.waiting() > 0 {
                arrived = true;
                break;
            }
            std::thread::sleep(Duration::from_millis(5));
        }
        switch();
        gate.release();
        arrived
    })
}

/// Join the switching thread and refuse to let the test claim anything unless
/// the race it describes actually happened: the request reached the gate, sat
/// there across the switch, and was let out by the test rather than by the
/// gate's own timeout.
fn assert_raced(switcher: std::thread::JoinHandle<bool>, gate: &'static Gate, what: &str) {
    let arrived = switcher.join().expect("the switching thread finished");
    // The held request records that it was let through from the stub's own
    // thread, which may not have woken yet: the switcher returns the instant
    // it releases the gate. Where the command under test is still blocked on
    // the answer this is already true; where it is not (the poll tick, which
    // nobody is waiting on) it is true a few milliseconds later.
    for _ in 0..400 {
        if gate.raced() || gate.timed_out() {
            break;
        }
        std::thread::sleep(Duration::from_millis(5));
    }
    assert!(
        arrived,
        "{what}: the request never reached the gate, so the switch happened with nothing in \
         flight and the discard path was never entered"
    );
    assert!(
        gate.raced(),
        "{what}: no request was held across the switch"
    );
    assert!(
        !gate.timed_out(),
        "{what}: the held request gave up waiting instead of being let through"
    );
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
    // No tick has been recorded against the new league yet. The first one
    // that is says the poller has moved on to it, which is what the poll
    // test below waits for.
    loaded.poll_last_success_at = None;
}

/// Wait until a poll tick has been recorded against the loaded league, or
/// give up after `timeout`. Answers whether one was.
fn wait_for_a_tick(loaded: &Mutex<Option<LoadedLeague>>, timeout: Duration) -> bool {
    let started = std::time::Instant::now();
    while started.elapsed() < timeout {
        {
            let guard = tauri::async_runtime::block_on(loaded.lock());
            if guard
                .as_ref()
                .is_some_and(|now| now.poll_last_success_at.is_some())
            {
                return true;
            }
        }
        std::thread::sleep(Duration::from_millis(20));
    }
    false
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
    SWITCH_PICKS.hold();
    let switched = loaded.clone();
    let switcher = switch_while_in_flight(&SWITCH_PICKS, move || {
        become_league(&switched, LEAGUE_ID, DRAFT_ID);
    });

    let error = s.err("refresh_picks", json!({}));
    assert_raced(switcher, &SWITCH_PICKS, "a pick refresh across a switch");
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
    TICK_PICKS.hold();
    let watched = loaded.clone();
    let switcher = switch_while_in_flight(&TICK_PICKS, move || {
        become_league(&watched, LEAGUE_ID, DRAFT_ID);
    });
    // The shortest interval there is, so the tick after the discarded one
    // comes round within the wait below.
    s.ok("start_polling", json!({"intervalSecs": 2}));
    assert_raced(switcher, &TICK_PICKS, "a poll tick across a switch");

    // The answer was let through the moment the switch landed, and the tick
    // that carried it records nothing on the new league. The tick after it
    // polls the new league's own draft and does record, so its arrival is
    // the proof that the discarded one has been and gone. Had the old picks
    // been written, that second tick's empty list would be refused against
    // them and never recorded, and this wait would run out.
    let ticked = wait_for_a_tick(&loaded, Duration::from_secs(15));
    s.ok("stop_polling", json!({}));
    assert!(ticked, "no tick was recorded against the new league");

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
    REBUILD_PICKS.hold();
    let switcher = switch_while_in_flight(&REBUILD_PICKS, move || {
        become_league(&loaded, LEAGUE_ID, DRAFT_ID);
        tauri::async_runtime::block_on(config.lock()).active_league_id =
            Some(LEAGUE_ID.to_string());
    });

    let error = s.err("refresh_data", json!({}));
    assert_raced(switcher, &REBUILD_PICKS, "a rebuild across a switch");
    assert!(error.contains("league changed"), "{error}");

    // The league the user chose is still the one on screen.
    let view = s.ok("get_state", json!({}));
    assert_eq!(view["league"]["league_id"], LEAGUE_ID);
    s.finish();
}

/// The judgement this keeps: a keeper is only recognisable while it sits
/// ahead of the clock, so the answer is made once, when the league is loaded,
/// and remembered. "Refresh projections" assembles a whole new `LoadedLeague`,
/// and that assembly makes the judgement again from where the clock stands
/// now, which mid-draft is a different and worse answer.
///
/// Through the command, not through `rebuild::carry_keepers` alone: the
/// helper was unit-tested and the call site was not, so deleting the call
/// left every test green and the bug back.
#[test]
fn a_rebuild_keeps_the_keeper_judgement_the_load_made() {
    let s = session("rebuild-keepers");
    s.ok("add_league", json!({"leagueId": LEAGUE_ID, "force": true}));
    let loaded = loaded_of(&s);
    // What the league on screen knows and the disk does not: a keeper noticed
    // at pick 5 whose save failed, and the floor the load set. The rebuild
    // reads the keeper file (which has neither) and an empty pick list, so
    // its own assembly would answer "no keepers, floor at pick 1".
    {
        let mut guard = tauri::async_runtime::block_on(loaded.lock());
        let league = guard.as_mut().expect("a league is loaded");
        league.keeper_pick_nos.picks.insert(5);
        league.keeper_pick_nos.floor = Some(2);
    }

    let rebuilt = s.ok("refresh_data", json!({}));
    assert_eq!(
        rebuilt["draft"]["keeper_picks"],
        json!([5]),
        "the rebuild made the keeper judgement again instead of carrying it"
    );
    let guard = tauri::async_runtime::block_on(loaded.lock());
    assert_eq!(
        guard
            .as_ref()
            .expect("a league is loaded")
            .keeper_pick_nos
            .floor,
        Some(2),
        "the keeper floor was re-derived from where the clock stands now"
    );
    drop(guard);
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
    LIVE_MATCHUPS.hold();
    let switcher = switch_while_in_flight(&LIVE_MATCHUPS, move || {
        become_league(&loaded, LEAGUE_ID, DRAFT_ID);
    });

    let error = s.err("refresh_season", json!({}));
    assert_raced(switcher, &LIVE_MATCHUPS, "a live refresh across a switch");
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

/// `/picks` dropping one row of a running draft moved the clock back to the
/// hole: the banner named the manager who had made that pick, and every pick
/// after it fell off the feed until the next answer.
#[test]
fn a_pick_list_with_a_hole_behind_the_clock_does_not_rewind_the_draft() {
    HOLE_PICKS.store(HOLE_FULL, Ordering::SeqCst);
    let s = session("holed-picks");
    let view = s.ok(
        "add_league",
        json!({"leagueId": LEAGUE_HOLE, "force": true}),
    );
    assert_eq!(view["draft"]["current_pick"], 5, "four picks are in");

    // The next answer is missing pick 2, with 3 and 4 still in it.
    HOLE_PICKS.store(HOLE_MISSING_ONE, Ordering::SeqCst);
    let error = s.err("refresh_picks", json!({}));
    assert!(
        error.contains("without pick 2") && error.contains("already on the board"),
        "the toast has to say what happened: {error}"
    );
    let kept = s.ok("get_state", json!({}));
    assert_eq!(
        kept["draft"]["current_pick"], 5,
        "a partial answer moved the clock back to the hole"
    );
    assert_eq!(
        kept["recent_picks"].as_array().expect("picks").len(),
        4,
        "the picks after the hole fell off the feed"
    );
    assert_eq!(kept["data_health"]["poll_consecutive_failures"], 1);

    // A commissioner taking the last pick back is a shorter list, not a
    // hole, and has to be adopted or the board never moves again.
    HOLE_PICKS.store(HOLE_LAST_UNDONE, Ordering::SeqCst);
    let undone = s.ok("refresh_picks", json!({}));
    assert_eq!(undone["draft"]["current_pick"], 4, "the undo was refused");
    assert_eq!(undone["data_health"]["poll_consecutive_failures"], 0);
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
