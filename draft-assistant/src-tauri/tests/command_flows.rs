//! The draft and season commands driven the way the frontend drives them.
//!
//! `tests/command_surface.rs` proves every command name is routed. This goes
//! the next step and runs real sessions through the IPC — add a league, take
//! a pick back, export the state, load the season, ask for the chat settings
//! — with Sleeper stood in for by `tests/stub`. What is under test is the
//! command layer itself: the argument handling, the order the locks are taken
//! in, and what each command hands back to the UI.

// The stub routes live beside this file rather than in `tests/stub`, so the
// other wire tests do not carry (and warn about) fixtures they never use.
#[path = "command_flows/routes.rs"]
mod routes;
mod stub;
#[path = "command_flows/switch_tests.rs"]
mod switch_tests;

use draft_assistant_lib::commands_chat as chat;
use draft_assistant_lib::commands_draft as draft;
use draft_assistant_lib::commands_season as season;
use draft_assistant_lib::engine::Engine;
use draft_assistant_lib::leagues;
use draft_assistant_lib::state::AppState;
use routes::{route, LEAGUE_ID, USER_ID};
use serde_json::{json, Value};
use std::sync::atomic::{AtomicBool, AtomicU64};
use std::sync::Arc;
use tauri::ipc::{CallbackFn, InvokeBody, InvokeResponseBody};
use tauri::test::{get_ipc_response, mock_builder, mock_context, noop_assets, INVOKE_KEY};
use tauri::webview::InvokeRequest;
use tauri::{Listener, Manager, WebviewWindowBuilder};
use tokio::sync::Mutex;

/// The app on Tauri's mock runtime, with the same state `lib.rs` installs and
/// an engine pointed at a scratch directory and the stub Sleeper.
struct Session {
    app: tauri::App<tauri::test::MockRuntime>,
    webview: tauri::WebviewWindow<tauri::test::MockRuntime>,
    data_dir: std::path::PathBuf,
}

fn session(label: &str) -> Session {
    stub::serve(route);
    let data_dir = stub::scratch_dir(label);
    let engine = Engine::new(data_dir.clone());
    let config = engine.load_config();
    let app = mock_builder()
        .invoke_handler(tauri::generate_handler![
            draft::add_league,
            draft::set_my_username,
            draft::get_config,
            leagues::sleeper_leagues,
            leagues::remove_league,
            draft::get_state,
            draft::refresh_picks,
            draft::refresh_data,
            draft::record_manual_pick,
            draft::undo_manual_pick,
            draft::export_state,
            draft::start_polling,
            draft::stop_polling,
            season::load_season,
            season::get_season,
            season::refresh_season,
            season::start_season_polling,
            season::stop_season_polling,
            season::headshot,
            chat::set_chat_provider,
            chat::set_chat_budget,
            chat::chat_settings,
        ])
        .build(mock_context(noop_assets()))
        .expect("the app builds on the mock runtime");
    app.manage(AppState {
        engine: Arc::new(engine),
        loaded: Arc::new(Mutex::new(None)),
        season: Arc::new(Mutex::new(None)),
        config: Arc::new(Mutex::new(config)),
        polling: Arc::new(AtomicBool::new(false)),
        poll_generation: Arc::new(AtomicU64::new(0)),
        season_polling: Arc::new(AtomicBool::new(false)),
        season_generation: Arc::new(AtomicU64::new(0)),
        last_season_view: Arc::new(Mutex::new(None)),
    });
    let webview = WebviewWindowBuilder::new(&app, "main", Default::default())
        .build()
        .expect("the main webview builds");
    Session {
        app,
        webview,
        data_dir,
    }
}

impl Session {
    /// One `invoke()` from the frontend, arguments and all.
    fn invoke(&self, cmd: &str, args: Value) -> Result<Value, String> {
        let response = get_ipc_response(
            &self.webview,
            InvokeRequest {
                cmd: cmd.to_string(),
                callback: CallbackFn(0),
                error: CallbackFn(1),
                url: "tauri://localhost".parse().expect("valid url"),
                body: InvokeBody::Json(args),
                headers: Default::default(),
                invoke_key: INVOKE_KEY.to_string(),
            },
        )
        // A command's `Err(String)` arrives as a JSON string; unwrap it so
        // assertions read against the message the user is shown.
        .map_err(|error| match error {
            Value::String(message) => message,
            other => other.to_string(),
        })?;
        Ok(match response {
            InvokeResponseBody::Json(json) => {
                serde_json::from_str(&json).expect("a command answered with valid JSON")
            }
            InvokeResponseBody::Raw(bytes) => json!(bytes),
        })
    }

    fn ok(&self, cmd: &str, args: Value) -> Value {
        self.invoke(cmd, args)
            .unwrap_or_else(|error| panic!("{cmd} failed: {error}"))
    }

    fn err(&self, cmd: &str, args: Value) -> String {
        self.invoke(cmd, args)
            .err()
            .unwrap_or_else(|| panic!("{cmd} was expected to fail"))
    }

    fn finish(self) {
        std::fs::remove_dir_all(&self.data_dir).ok();
        drop(self.webview);
        drop(self.app);
    }
}

#[test]
fn a_session_adds_a_league_and_the_board_comes_back_to_the_screen() {
    let s = session("add-league");
    let view = s.ok("add_league", json!({"leagueId": LEAGUE_ID, "force": true}));
    assert_eq!(view["league"]["name"], "Command League");
    let available = view["available"]
        .as_array()
        .expect("the view carries the board");
    assert!(!available.is_empty());
    assert_eq!(view["data_health"]["board_size"], available.len());

    // The league is remembered and made active, so the next launch reopens it.
    let config = s.ok("get_config", json!({}));
    assert_eq!(config["active_league_id"], LEAGUE_ID);
    assert_eq!(config["leagues"][0]["name"], "Command League");

    // get_state re-renders from what is already loaded, without refetching.
    let again = s.ok("get_state", json!({}));
    assert_eq!(again["league"]["name"], "Command League");
    s.finish();
}

#[test]
fn text_that_is_not_a_sleeper_id_is_refused_before_any_request() {
    let s = session("bad-id");
    let error = s.err("add_league", json!({"leagueId": "../players/nfl"}));
    assert!(error.contains("doesn't look like a Sleeper ID"), "{error}");
    // A URL with the id in it is fine, though: that is what people paste.
    let view = s.ok(
        "add_league",
        json!({"leagueId": format!("https://sleeper.com/leagues/{LEAGUE_ID}/team")}),
    );
    assert_eq!(view["league"]["name"], "Command League");
    s.finish();
}

#[test]
fn nothing_that_needs_a_league_answers_before_one_is_loaded() {
    let s = session("empty");
    for command in ["get_state", "refresh_picks", "export_state", "load_season"] {
        let error = s.err(command, json!({}));
        assert!(
            error.contains("no league loaded"),
            "{command} said: {error}"
        );
    }
    assert!(s
        .err("refresh_data", json!({}))
        .contains("no active league"));
    s.finish();
}

#[test]
fn a_manual_pick_is_taken_at_the_clock_and_can_be_taken_back() {
    let s = session("manual-pick");
    s.ok("add_league", json!({"leagueId": LEAGUE_ID, "force": true}));

    let view = s.ok("record_manual_pick", json!({"playerId": "rb-1"}));
    let picks = view["recent_picks"]
        .as_array()
        .expect("the view carries the picks so far");
    assert_eq!(picks.len(), 1);
    assert_eq!(picks[0]["player_id"], "rb-1");
    assert_eq!(picks[0]["pick_no"], 1, "the first open slot on the board");
    assert!(
        !view["available"]
            .as_array()
            .expect("board")
            .iter()
            .any(|p| p["player_id"] == "rb-1"),
        "a drafted player leaves the available list"
    );

    // The same player twice is a mis-tap, not a second pick.
    assert_eq!(
        s.err("record_manual_pick", json!({"playerId": "rb-1"})),
        "player already drafted"
    );
    // An id that is not on this league's board would be written to disk and
    // reloaded as a ghost pick.
    let error = s.err("record_manual_pick", json!({"playerId": "not-a-player"}));
    assert!(error.contains("not on this league's board"), "{error}");

    let undone = s.ok("undo_manual_pick", json!({}));
    assert!(undone["recent_picks"].as_array().expect("picks").is_empty());
    let error = s.err("undo_manual_pick", json!({}));
    assert!(error.contains("no manual picks to undo"), "{error}");
    s.finish();
}

#[test]
fn the_exported_state_is_the_same_view_the_screen_is_showing() {
    let s = session("export");
    let view = s.ok("add_league", json!({"leagueId": LEAGUE_ID, "force": true}));
    let path = s.ok("export_state", json!({}));
    let path = path.as_str().expect("a path came back");
    let written: Value =
        serde_json::from_str(&std::fs::read_to_string(path).expect("the file exists"))
            .expect("the export is JSON");
    assert_eq!(written["league"], view["league"]);
    assert_eq!(written["available"], view["available"]);
    assert_eq!(written["schema_version"], view["schema_version"]);
    s.finish();
}

#[test]
fn a_refresh_re_reads_the_picks_and_the_draft_status() {
    let s = session("refresh");
    s.ok("add_league", json!({"leagueId": LEAGUE_ID, "force": true}));
    let refreshed = s.ok("refresh_picks", json!({}));
    assert_eq!(refreshed["draft"]["status"], "drafting");
    // A full rebuild goes back to the wire for everything and must still come
    // back with the same board.
    let rebuilt = s.ok("refresh_data", json!({}));
    assert_eq!(rebuilt["available"], refreshed["available"]);
    s.finish();
}

#[test]
fn saving_a_username_unlocks_the_league_picker() {
    let s = session("username");
    let error = s.err("sleeper_leagues", json!({}));
    assert!(error.contains("no Sleeper account saved"), "{error}");

    let user_id = s.ok("set_my_username", json!({"username": "ada"}));
    assert_eq!(user_id, USER_ID);
    // Without a loaded league there is no season to look leagues up for.
    let error = s.err("sleeper_leagues", json!({}));
    assert!(error.contains("no league loaded"), "{error}");

    let leagues = s.ok("sleeper_leagues", json!({"season": "2026"}));
    assert_eq!(leagues[0]["league_id"], LEAGUE_ID);
    assert_eq!(leagues[0]["name"], "Command League");
    // The picker says where each league is, so a draft night is easy to spot.
    assert_eq!(leagues[0]["status"], "drafting");
    s.finish();
}

#[test]
fn a_league_can_be_forgotten_but_not_while_it_is_on_screen() {
    let s = session("forget-league");
    s.ok("add_league", json!({"leagueId": LEAGUE_ID, "force": true}));
    let config = s.ok("get_config", json!({}));
    assert_eq!(config["leagues"][0]["status"], "drafting");

    let error = s.err("remove_league", json!({"leagueId": LEAGUE_ID}));
    assert!(error.contains("on screen"), "{error}");
    let error = s.err("remove_league", json!({"leagueId": "9999999999999999999"}));
    assert!(error.contains("not in the list"), "{error}");

    // Forgetting works on the list, not the screen: the loaded league stays.
    {
        let state = s.app.state::<AppState>();
        let mut config = tauri::async_runtime::block_on(state.config.lock());
        config.active_league_id = None;
    }
    let left = s.ok("remove_league", json!({"leagueId": LEAGUE_ID}));
    assert_eq!(left, json!([]));
    assert_eq!(s.ok("get_config", json!({}))["leagues"], json!([]));
    assert_eq!(
        s.ok("get_state", json!({}))["league"]["name"],
        "Command League"
    );
    s.finish();
}

#[test]
fn the_season_screen_loads_refreshes_and_re_renders() {
    let s = session("season");
    s.ok("add_league", json!({"leagueId": LEAGUE_ID, "force": true}));
    assert!(s.err("get_season", json!({})).contains("not loaded"));

    let view = s.ok("load_season", json!({"force": true}));
    assert_eq!(view["week"], 1);
    let standings = view["standings"].as_array().expect("a standings table");
    assert_eq!(standings.len(), 2);

    // get_season re-renders what is held; refresh_season re-pulls the live
    // slice. Both answer with the same week.
    assert_eq!(s.ok("get_season", json!({}))["week"], 1);
    assert_eq!(s.ok("refresh_season", json!({}))["week"], 1);
    s.finish();
}

#[test]
fn a_player_with_no_photo_is_a_null_rather_than_an_error() {
    let s = session("headshot");
    // The CDN is not the stub host, so this cannot reach anything; a missing
    // photo must still leave the roster renderable.
    let answer = s.ok("headshot", json!({"playerId": "qb-1"}));
    assert!(answer.is_null(), "{answer}");
    s.finish();
}

#[test]
fn the_chat_settings_report_what_this_machine_can_actually_do() {
    let s = session("chat-settings");
    let settings = s.ok("chat_settings", json!({}));
    assert!(settings["budget_usd"].is_number());
    assert!(settings["spend_usd"].is_object(), "spend is per screen");
    assert!(settings["models"].as_array().is_some_and(|m| !m.is_empty()));
    assert!(["keychain", "file"].contains(&settings["key_store"].as_str().expect("a store")));

    // A budget is a positive number of dollars; anything else means "no cap".
    assert_eq!(s.ok("set_chat_budget", json!({"dollars": 12.5})), 12.5);
    assert_eq!(s.ok("set_chat_budget", json!({"dollars": -3.0})), 0.0);
    assert_eq!(s.ok("set_chat_budget", json!({"dollars": 0.0})), 0.0);

    assert_eq!(s.ok("set_chat_provider", json!({"provider": "api"})), "api");
    assert!(!s
        .err("set_chat_provider", json!({"provider": "carrier pigeon"}))
        .is_empty());

    // Settings survive the round trip to disk.
    let config = s.ok("get_config", json!({}));
    assert_eq!(config["chat_budget_usd"], 0.0);
    assert_eq!(config["chat_provider"], "api");
    s.finish();
}

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
