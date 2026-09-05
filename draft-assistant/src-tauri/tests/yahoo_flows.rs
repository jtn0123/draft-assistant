//! The Yahoo commands driven the way the frontend drives them.
//!
//! `tests/yahoo_wire.rs` proves the client speaks Yahoo's dialect and
//! `tests/yahoo_auth_wire.rs` proves the token dance; this goes the last step
//! and runs whole sessions through the IPC on Tauri's mock runtime — save the
//! credentials, connect, list the leagues, add one by key and by URL, build
//! the board, take a tick's worth of new picks, disconnect.
//!
//! Two stubs stand in for the outside world: `tests/yahoo_stub` serves both
//! Yahoo hosts from one socket (the app's `YahooState` is pointed at it), and
//! `tests/stub` serves Sleeper, whose players dictionary and projections are
//! still where every number on the board comes from.
//!
//! Nothing here touches a Keychain or opens a browser: the session's
//! `YahooState::sandboxed` puts the secrets in a file under the scratch data
//! directory and leaves the browser alone.

mod stub;
mod yahoo_stub;

#[path = "yahoo_flows/harness.rs"]
mod harness;

use harness::{session, AUCTION_KEY, CLIENT_ID, CODE, LEAGUE_KEY, SECRET};
use serde_json::{json, Value};
use std::sync::atomic::Ordering;
use std::sync::Arc;
use tauri::Listener;

#[test]
fn credentials_are_stored_and_the_status_says_so_without_handing_them_back() {
    let s = session("yahoo-credentials");
    let before = s.ok("yahoo_status", json!({}));
    assert_eq!(before["configured"], false);
    assert_eq!(before["connected"], false);
    assert_eq!(before["redirect"], "oob");
    assert_eq!(before["account"], Value::Null);

    let after = s.ok(
        "yahoo_save_credentials",
        json!({"clientId": CLIENT_ID, "clientSecret": SECRET}),
    );
    assert_eq!(after["configured"], true);
    assert_eq!(after["connected"], false);
    // The secret never comes back over the IPC, in any field.
    let text = after.to_string();
    assert!(
        !text.contains(SECRET),
        "the status carried the secret: {text}"
    );
    assert!(
        !text.contains(CLIENT_ID),
        "the status carried the id: {text}"
    );

    // Half a pair is not a pair.
    let error = s.err(
        "yahoo_save_credentials",
        json!({"clientId": CLIENT_ID, "clientSecret": "  "}),
    );
    assert!(error.contains("client secret"), "{error}");
    s.finish();
}

#[test]
fn connect_cannot_start_before_the_app_is_registered() {
    let s = session("yahoo-unconfigured");
    let error = s.err("yahoo_begin_connect", json!({}));
    assert!(error.contains("client id"), "{error}");
    // …and nothing can be loaded either.
    let error = s.err("yahoo_leagues", json!({}));
    assert!(error.contains("Yahoo is not set up"), "{error}");
    s.finish();
}

#[test]
fn the_authorize_url_carries_the_client_id_the_redirect_and_the_state() {
    let s = session("yahoo-begin");
    s.ok(
        "yahoo_save_credentials",
        json!({"clientId": CLIENT_ID, "clientSecret": SECRET}),
    );
    let start = s.ok("yahoo_begin_connect", json!({}));
    let url = start["authorize_url"].as_str().expect("a URL to open");
    assert!(url.contains("/oauth2/request_auth"), "{url}");
    assert!(url.contains(&format!("client_id={CLIENT_ID}")), "{url}");
    assert!(url.contains("redirect_uri=oob"), "{url}");
    let state = start["state"].as_str().expect("a state");
    assert!(url.contains(&format!("state={state}")), "{url}");
    assert_eq!(start["redirect"], "oob");
    // The secret is never in the browser's address bar.
    assert!(!url.contains(SECRET), "{url}");
    s.finish();
}

#[test]
fn a_code_from_a_different_sign_in_is_refused_and_burns_the_one_in_progress() {
    let s = session("yahoo-state-check");
    s.ok(
        "yahoo_save_credentials",
        json!({"clientId": CLIENT_ID, "clientSecret": SECRET}),
    );
    s.ok("yahoo_begin_connect", json!({}));
    let error = s.err(
        "yahoo_finish_connect",
        json!({"code": CODE, "state": "not-the-one"}),
    );
    assert!(error.contains("different sign-in"), "{error}");
    // The expected state was consumed with it: a second attempt has to start
    // Connect again rather than replay a code against a stale value.
    let error = s.err("yahoo_finish_connect", json!({"code": CODE, "state": "x"}));
    assert!(error.contains("no Yahoo sign-in"), "{error}");
    assert_eq!(s.ok("yahoo_status", json!({}))["connected"], false);
    s.finish();
}

#[test]
fn a_mistyped_code_can_be_corrected_without_going_round_the_browser_again() {
    let s = session("yahoo-bad-code");
    s.ok(
        "yahoo_save_credentials",
        json!({"clientId": CLIENT_ID, "clientSecret": SECRET}),
    );
    let start = s.ok("yahoo_begin_connect", json!({}));
    let state = start["state"].as_str().expect("a state").to_string();

    // Yahoo refuses the code. The sign-in is still the user's, so the message
    // says what to do and does not blame a sign-in that is not in progress.
    let error = s.err(
        "yahoo_finish_connect",
        json!({"code": "typo-abcd", "state": state}),
    );
    assert!(error.contains("rejected that code"), "{error}");
    assert!(!error.contains("no Yahoo sign-in"), "{error}");
    assert_eq!(s.ok("yahoo_status", json!({}))["connected"], false);

    // The same sign-in, the right code: no second trip to the browser.
    let status = s.ok(
        "yahoo_finish_connect",
        json!({"code": CODE, "state": state}),
    );
    assert_eq!(status["connected"], true);
    s.finish();
}

#[test]
fn a_finished_sign_in_leaves_the_account_connected() {
    let s = session("yahoo-connect");
    let status = s.connect();
    assert_eq!(status["configured"], true);
    assert_eq!(status["connected"], true);
    // Read back cold, so it is the stored pair that is being seen.
    assert_eq!(s.ok("yahoo_status", json!({}))["connected"], true);
    s.finish();
}

#[test]
fn the_accounts_leagues_come_back_named_sorted_and_marked_as_yahoos() {
    let s = session("yahoo-leagues");
    s.connect();
    let leagues = s.ok("yahoo_leagues", json!({}));
    let rows = leagues.as_array().expect("a list of leagues");
    assert_eq!(rows.len(), 2);
    let names: Vec<&str> = rows.iter().map(|l| l["name"].as_str().unwrap()).collect();
    let mut sorted = names.clone();
    sorted.sort_by_key(|name| name.to_lowercase());
    assert_eq!(names, sorted, "the picker's list is not in reading order");
    assert!(rows.iter().all(|l| l["platform"] == "yahoo"));
    assert!(rows
        .iter()
        .any(|l| l["league_id"] == LEAGUE_KEY && l["season"] == "2026"));
    s.finish();
}

#[test]
fn a_yahoo_league_key_builds_a_board_out_of_sleepers_numbers() {
    let s = session("yahoo-add-key");
    s.connect();
    let view = s.ok("add_league", json!({"leagueId": LEAGUE_KEY, "force": true}));
    assert_eq!(view["league"]["league_id"], LEAGUE_KEY);
    assert_eq!(view["league"]["platform"], "yahoo");
    assert_eq!(view["league"]["name"], "Wire Wednesday");
    assert_eq!(view["league"]["season"], "2026");
    assert_eq!(view["league"]["total_rosters"], 12);
    // Yahoo's `W/R/T` is the app's FLEX by the time the board sees it.
    let slots = view["league"]["roster_positions"]
        .as_array()
        .expect("the roster shape");
    assert!(slots.iter().any(|slot| slot == "FLEX"), "{slots:?}");
    assert!(slots.iter().any(|slot| slot == "SUPER_FLEX"), "{slots:?}");
    // 16 seats drafted into: the two IR slots are not.
    assert_eq!(view["draft"]["rounds"], 16);
    // The settings fixture's league has not started drafting yet, and Yahoo's
    // `predraft` is the app's `pre_draft`.
    assert_eq!(view["draft"]["status"], "pre_draft");

    // The picks Yahoo has recorded are on the board, under Sleeper's ids
    // where the crosswalk found one.
    let taken: Vec<&str> = view["recent_picks"]
        .as_array()
        .expect("picks")
        .iter()
        .map(|pick| pick["player_id"].as_str().unwrap())
        .collect();
    assert!(
        taken.contains(&"6794"),
        "Ja'Marr Chase is not crossed over: {taken:?}"
    );
    assert!(
        taken.iter().any(|id| id.starts_with("yahoo:")),
        "a player with no Sleeper row lost his pick: {taken:?}"
    );
    // The team flagged as the current login is mine, so my roster resolves
    // without a Sleeper user id anywhere in sight.
    assert_eq!(view["draft"]["my_slot"], 1);
    assert_eq!(view["my_roster"]["slot"], 1);

    // Bijan Robinson is in Yahoo's pool and not in Sleeper's dictionary.
    let warnings = view["data_health"]["warnings"].to_string();
    assert!(
        warnings.contains("had no Sleeper match"),
        "the unmatched player was not reported: {warnings}"
    );

    // Now that a team list is cached, the settings panel can name the
    // manager without going back to Yahoo for it.
    assert_eq!(s.ok("yahoo_status", json!({}))["account"], "Ada");

    // …and the league is remembered as a Yahoo one for the next launch.
    let config = s.ok("get_config", json!({}));
    assert_eq!(config["active_league_id"], LEAGUE_KEY);
    assert_eq!(config["leagues"][0]["platform"], "yahoo");
    s.finish();
}

#[test]
fn a_reload_serves_the_player_pool_from_disk_rather_than_paging_yahoo_again() {
    let s = session("yahoo-pool-cache");
    s.connect();
    s.ok("add_league", json!({"leagueId": LEAGUE_KEY, "force": true}));
    let first = s.pool_calls.load(Ordering::SeqCst);
    assert!(first > 0, "the pool was never fetched at all");

    // Same league again, unforced. The pool is the expensive call — 25 rows a
    // page — and its cache is still fresh, so not one page goes out.
    s.ok("add_league", json!({"leagueId": LEAGUE_KEY}));
    assert_eq!(
        s.pool_calls.load(Ordering::SeqCst),
        first,
        "the cached player pool was fetched again"
    );

    // Forcing is still the way to go back to Yahoo for it.
    s.ok("add_league", json!({"leagueId": LEAGUE_KEY, "force": true}));
    assert!(
        s.pool_calls.load(Ordering::SeqCst) > first,
        "a forced refresh did not refetch the pool"
    );
    s.finish();
}

#[test]
fn a_pasted_yahoo_league_url_is_resolved_to_the_key_the_api_wants() {
    let s = session("yahoo-add-url");
    s.connect();
    let view = s.ok(
        "add_league",
        json!({"leagueId": "https://football.fantasysports.yahoo.com/f1/12345"}),
    );
    assert_eq!(view["league"]["league_id"], LEAGUE_KEY);
    assert_eq!(view["league"]["platform"], "yahoo");

    // A league the account does not play in cannot be resolved, and the
    // message says what to do about it.
    let error = s.err(
        "add_league",
        json!({"leagueId": "https://football.fantasysports.yahoo.com/f1/99999"}),
    );
    assert!(error.contains("99999"), "{error}");
    assert!(error.contains("Yahoo account"), "{error}");
    s.finish();
}

#[test]
fn adding_a_yahoo_league_before_connecting_says_to_connect() {
    let s = session("yahoo-add-cold");
    let error = s.err("add_league", json!({"leagueId": LEAGUE_KEY}));
    assert!(error.contains("Yahoo is not set up"), "{error}");
    s.finish();
}

#[test]
fn a_refresh_takes_the_picks_made_since_the_board_was_built() {
    let s = session("yahoo-refresh");
    s.connect();
    let view = s.ok("add_league", json!({"leagueId": LEAGUE_KEY, "force": true}));
    let before = view["draft"]["total_picks_made"].as_u64().expect("a count");
    assert_eq!(before, 3);

    s.advanced.store(true, Ordering::SeqCst);
    let view = s.ok("refresh_picks", json!({}));
    let after = view["draft"]["total_picks_made"].as_u64().expect("a count");
    assert!(after > before, "the new picks were not applied: {after}");
    // The tick reported no trouble, which is what the health strip reads.
    assert_eq!(view["data_health"]["poll_last_error"], Value::Null);
    assert_eq!(view["data_health"]["poll_consecutive_failures"], 0);
    s.finish();
}

#[test]
fn the_background_poller_applies_new_yahoo_picks_and_says_so() {
    let s = session("yahoo-poll");
    s.connect();
    s.ok("add_league", json!({"leagueId": LEAGUE_KEY, "force": true}));

    let seen: Arc<std::sync::Mutex<Vec<u64>>> = Arc::new(std::sync::Mutex::new(Vec::new()));
    let recorder = seen.clone();
    s.app.listen("draft-updated", move |event| {
        if let Ok(view) = serde_json::from_str::<Value>(event.payload()) {
            if let Some(total) = view["draft"]["total_picks_made"].as_u64() {
                recorder.lock().expect("recorder").push(total);
            }
        }
    });

    s.advanced.store(true, Ordering::SeqCst);
    s.ok("start_polling", json!({"intervalSecs": 2}));
    let mut applied = None;
    for _ in 0..100 {
        std::thread::sleep(std::time::Duration::from_millis(100));
        if let Some(total) = seen.lock().expect("recorder").last().copied() {
            applied = Some(total);
            break;
        }
    }
    s.ok("stop_polling", json!({}));
    assert_eq!(
        applied,
        Some(6),
        "the poll tick never emitted the finished draft"
    );
    s.finish();
}

#[test]
fn disconnecting_signs_out_but_keeps_the_registered_app() {
    let s = session("yahoo-disconnect");
    s.connect();
    s.ok("yahoo_leagues", json!({}));

    let status = s.ok("yahoo_disconnect", json!({}));
    assert_eq!(status["connected"], false);
    // The app the user registered is not the account they signed out of.
    assert_eq!(status["configured"], true);
    assert_eq!(s.ok("yahoo_status", json!({}))["connected"], false);
    // The client built while connected is thrown away with the tokens.
    let error = s.err("yahoo_leagues", json!({}));
    assert!(error.contains("not connected to Yahoo"), "{error}");

    // …and reconnecting is the browser trip alone: no credentials to re-paste.
    let start = s.ok("yahoo_begin_connect", json!({}));
    let state = start["state"].as_str().expect("a state");
    let status = s.ok(
        "yahoo_finish_connect",
        json!({"code": CODE, "state": state}),
    );
    assert_eq!(status["connected"], true);
    s.finish();
}

#[test]
fn forgetting_the_app_credentials_is_a_second_deliberate_step() {
    let s = session("yahoo-forget");
    s.connect();

    let status = s.ok("yahoo_disconnect", json!({"forgetCredentials": true}));
    assert_eq!(status["connected"], false);
    assert_eq!(status["configured"], false);
    // With no app registered there is nothing left to sign in with.
    let error = s.err("yahoo_begin_connect", json!({}));
    assert!(error.contains("client id"), "{error}");
    let error = s.err("yahoo_leagues", json!({}));
    assert!(error.contains("Yahoo is not set up"), "{error}");
    s.finish();
}

#[test]
fn an_auction_league_answers_with_its_budget_and_what_each_pick_cost() {
    // The failure this prevents: an auction was detected and then nothing was
    // ever done with the money — no budget was parsed and the costs Yahoo
    // sends on every pick reached nothing that could show them.
    let s = session("yahoo-auction");
    s.connect();
    let auction = s.ok("yahoo_auction", json!({"leagueKey": AUCTION_KEY}));
    assert_eq!(auction["budget"], 200);
    assert_eq!(auction["costs"]["yahoo:30977"], 55.0);
    assert_eq!(auction["costs"]["yahoo:31883"], 41.0);
    assert_eq!(
        auction["costs"].as_object().expect("the costs").len(),
        3,
        "one cost per filled pick"
    );

    // A snake league answers with the same shape and nothing in it, so the
    // caller needs no separate question about the draft type.
    let snake = s.ok("yahoo_auction", json!({"leagueKey": LEAGUE_KEY}));
    assert_eq!(snake["budget"], Value::Null);
    assert!(snake["costs"].as_object().expect("the costs").is_empty());
    s.finish();
}

#[test]
fn a_player_kept_rather_than_drafted_is_marked_as_kept_and_stays_marked() {
    // The failure this prevents: keepers were read off `draftresults`, and the
    // live resource does not send `is_keeper` there at all. Every kept player
    // was drawn as an ordinary first-round pick, so the board took him off at
    // the wrong moment and the pick maths counted a pick nobody would make.
    // Yahoo does say on the roster, and `teams;out=roster` fetches every
    // team's in one call.
    let s = session("yahoo-keepers");
    s.connect();
    let view = s.ok("add_league", json!({"leagueId": LEAGUE_KEY, "force": true}));
    assert_eq!(
        view["draft"]["keeper_picks"],
        json!([3]),
        "the kept player on the rosters payload was not recognised"
    );
    let kept = view["rosters"]
        .as_array()
        .expect("the rosters")
        .iter()
        .flat_map(|roster| roster["players"].as_array().expect("a roster").iter())
        .find(|player| player["pick_no"] == 3)
        .expect("pick three landed on somebody's roster");
    assert_eq!(kept["is_keeper"], true, "kept, not drafted tonight");
    // The two players Yahoo did not flag are ordinary picks, not keepers by
    // omission.
    assert_eq!(
        view["draft"]["total_picks_made"], 3,
        "a keeper is still a pick that has happened"
    );

    // …and a tick does not take it back. The poller re-reads the draft alone,
    // so the flags have to survive on the load's own caches.
    let view = s.ok("refresh_picks", json!({}));
    assert_eq!(view["draft"]["keeper_picks"], json!([3]));
    assert_eq!(view["data_health"]["poll_last_error"], Value::Null);
    s.finish();
}

#[test]
fn the_board_says_out_loud_that_it_has_not_read_yahoos_traded_picks() {
    // A Yahoo league that trades draft picks is drawn in plain snake order
    // with the picks in the wrong hands. It looks right and it is wrong, which
    // is the worst way for a board to be wrong, so every load says so.
    let s = session("yahoo-traded-picks");
    s.connect();
    let view = s.ok("add_league", json!({"leagueId": LEAGUE_KEY, "force": true}));
    let warnings = view["data_health"]["warnings"].to_string();
    assert!(
        warnings.contains("traded picks are not read"),
        "the board did not say pick ownership is guessed: {warnings}"
    );
    s.finish();
}
