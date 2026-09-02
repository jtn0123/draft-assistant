//! The draft and season commands driven the way the frontend drives them.
//!
//! `tests/command_surface.rs` proves every command name is routed. This goes
//! the next step and runs real sessions through the IPC — add a league, take
//! a pick back, export the state, load the season, ask for the chat settings
//! — with Sleeper stood in for by `tests/stub`. What is under test is the
//! command layer itself: the argument handling, the order the locks are taken
//! in, and what each command hands back to the UI.

mod stub;

use draft_assistant_lib::commands_chat as chat;
use draft_assistant_lib::commands_draft as draft;
use draft_assistant_lib::commands_season as season;
use draft_assistant_lib::engine::Engine;
use draft_assistant_lib::leagues;
use draft_assistant_lib::state::AppState;
use serde_json::{json, Value};
use std::sync::atomic::{AtomicBool, AtomicU64};
use std::sync::Arc;
use tauri::ipc::{CallbackFn, InvokeBody, InvokeResponseBody};
use tauri::test::{get_ipc_response, mock_builder, mock_context, noop_assets, INVOKE_KEY};
use tauri::webview::InvokeRequest;
use tauri::{Manager, WebviewWindowBuilder};
use tokio::sync::Mutex;

/// A Sleeper league id is a long run of digits, and `add_league` insists on
/// one, so the fixture uses a realistic id rather than "league-1".
const LEAGUE_ID: &str = "1000000000000000001";
const DRAFT_ID: &str = "2000000000000000002";
const USER_ID: &str = "3000000000000000003";

fn league_json() -> String {
    format!(
        r#"{{"league_id": "{LEAGUE_ID}", "name": "Command League", "season": "2026",
             "status": "drafting", "total_rosters": 2,
             "roster_positions": ["QB", "RB", "WR", "TE", "FLEX", "BN"],
             "scoring_settings": {{"rec": 1.0, "rush_yd": 0.1, "rush_td": 6.0,
                                   "rec_yd": 0.1, "rec_td": 6.0, "pass_yd": 0.04,
                                   "pass_td": 4.0}},
             "draft_id": "{DRAFT_ID}", "settings": {{"playoff_week_start": 15}}}}"#
    )
}

const PLAYERS: &str = r#"{
    "qb-1": {"full_name": "Command Passer", "position": "QB", "team": "AAA"},
    "rb-1": {"full_name": "Command Runner", "position": "RB", "team": "BBB"},
    "wr-1": {"full_name": "Command Catcher", "position": "WR", "team": "CCC"}
}"#;

const SEASON_ROWS: &str = r#"[
    {"player_id": "qb-1", "stats": {"pass_yd": 4200.0, "pass_td": 32.0, "adp_ppr": 14.0},
     "player": {"position": "QB", "team": "AAA"}},
    {"player_id": "rb-1", "stats": {"rush_yd": 1200.0, "rush_td": 10.0, "rec": 40.0, "adp_ppr": 3.0},
     "player": {"position": "RB", "team": "BBB"}},
    {"player_id": "wr-1", "stats": {"rec_yd": 1300.0, "rec_td": 9.0, "rec": 95.0, "adp_ppr": 5.0},
     "player": {"position": "WR", "team": "CCC"}}
]"#;

const ROSTERS: &str = r#"[
    {"roster_id": 1, "owner_id": "3000000000000000003", "players": ["qb-1", "rb-1"],
     "starters": ["qb-1"], "settings": {"wins": 1, "losses": 0, "fpts": 120}},
    {"roster_id": 2, "owner_id": "other", "players": ["wr-1"],
     "starters": ["wr-1"], "settings": {"wins": 0, "losses": 1, "fpts": 90}}
]"#;

fn route(path: &str) -> Option<stub::Reply> {
    let path = path.split('?').next().unwrap_or(path);
    let ok = |body: String| Some((200u16, body));
    if path == "/v1/players/nfl" {
        return ok(PLAYERS.to_string());
    }
    if path == "/v1/state/nfl" {
        return ok(r#"{"season": "2026", "week": 1, "display_week": 1}"#.to_string());
    }
    if path.starts_with("/scores/nfl/") {
        return ok("[]".to_string());
    }
    if let Some(rest) = path.strip_prefix("/projections/nfl/2026") {
        return match rest.is_empty() {
            true => ok(SEASON_ROWS.to_string()),
            false => ok("[]".to_string()),
        };
    }
    if let Some(rest) = path.strip_prefix(&format!("/v1/league/{LEAGUE_ID}")) {
        return match rest {
            "" => ok(league_json()),
            "/users" => ok(format!(
                r#"[{{"user_id": "{USER_ID}", "display_name": "Ada"}}]"#
            )),
            "/rosters" => ok(ROSTERS.to_string()),
            "/winners_bracket" => ok("[]".to_string()),
            _ if rest.starts_with("/matchups") || rest.starts_with("/transactions") => {
                ok("[]".to_string())
            }
            _ => None,
        };
    }
    if let Some(rest) = path.strip_prefix(&format!("/v1/draft/{DRAFT_ID}")) {
        return match rest {
            "" => ok(format!(
                r#"{{"draft_id": "{DRAFT_ID}", "status": "drafting", "type": "snake",
                     "settings": {{"teams": 2, "rounds": 3}},
                     "draft_order": {{"{USER_ID}": 1}}, "season": "2026"}}"#
            )),
            "/picks" | "/traded_picks" => ok("[]".to_string()),
            _ => None,
        };
    }
    match path {
        "/v1/user/ada" => ok(format!(
            r#"{{"user_id": "{USER_ID}", "username": "ada", "display_name": "Ada"}}"#
        )),
        _ if path.starts_with(&format!("/v1/user/{USER_ID}/leagues/nfl/")) => {
            ok(format!("[{}]", league_json()))
        }
        _ => None,
    }
}

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
            draft::get_state,
            draft::refresh_picks,
            draft::refresh_data,
            draft::record_manual_pick,
            draft::undo_manual_pick,
            draft::export_state,
            draft::stop_polling,
            season::load_season,
            season::get_season,
            season::refresh_season,
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
