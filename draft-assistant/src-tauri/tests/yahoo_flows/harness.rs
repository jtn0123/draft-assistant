//! The stubs and the mock-runtime session `tests/yahoo_flows.rs` runs on.
//!
//! Beside the tests rather than inside them so the file that reads as a list
//! of behaviours stays one, and so the two stub routers — Yahoo's two hosts on
//! one socket, Sleeper's players and projections on another — sit together.

use crate::stub;
use crate::yahoo_stub::{self, Reply, Request, Stub};
use draft_assistant_lib::commands_draft as draft;
use draft_assistant_lib::commands_yahoo as yahoo;
use draft_assistant_lib::engine::Engine;
use draft_assistant_lib::state::{AppState, YahooState};
use draft_assistant_lib::yahoo::YahooHosts;
use serde_json::{json, Value};
use std::sync::atomic::{AtomicBool, AtomicU64, Ordering};
use std::sync::Arc;
use tauri::ipc::{CallbackFn, InvokeBody, InvokeResponseBody};
use tauri::test::{get_ipc_response, mock_builder, mock_context, noop_assets, INVOKE_KEY};
use tauri::webview::InvokeRequest;
use tauri::{Manager, WebviewWindowBuilder};
use tokio::sync::Mutex;

const USER_LEAGUES: &str = include_str!("../fixtures/yahoo/user_leagues.json");
const LEAGUE: &str = include_str!("../fixtures/yahoo/league_settings.json");
const TEAMS: &str = include_str!("../fixtures/yahoo/teams.json");
const ROSTERS: &str = include_str!("../fixtures/yahoo/teams_rosters.json");
const PARTIAL: &str = include_str!("../fixtures/yahoo/draft_results_partial.json");
const COMPLETE: &str = include_str!("../fixtures/yahoo/draft_results_complete.json");
const PLAYERS_0: &str = include_str!("../fixtures/yahoo/players_page_0.json");
const AUCTION_LEAGUE: &str = include_str!("../fixtures/yahoo/league_settings_auction.json");
const AUCTION_RESULTS: &str = include_str!("../fixtures/yahoo/draft_results_auction.json");

pub(crate) const LEAGUE_KEY: &str = "449.l.12345";
/// The second league on the account, which is the auction one.
pub(crate) const AUCTION_KEY: &str = "449.l.67890";
pub(crate) const CLIENT_ID: &str = "dj0yJmk9flowsclient";
pub(crate) const SECRET: &str = "flows-client-secret";
pub(crate) const CODE: &str = "abcd1234";

/// Yahoo's two hosts, from one socket. The token endpoint answers whatever a
/// well-behaved Yahoo would: an hour-long access token and a refresh token.
///
/// `advanced` belongs to one session rather than to the file: set it and that
/// session's stub starts serving the finished draft instead of the partial
/// one. Per-session so the two tests that use it can run side by side, which
/// is how `cargo test` runs them.
fn yahoo_route(request: &Request, advanced: &AtomicBool, pool_calls: &AtomicU64) -> Reply {
    let path = request.path();
    if path.ends_with("/oauth2/get_token") {
        // The exchange must carry the app's identity, and the code has to be
        // the one Yahoo issued — a mistyped code is answered the way Yahoo
        // answers one, so the retry path is exercised rather than assumed.
        let identified = request.header("authorization").is_some();
        let granted = match request.form("code") {
            Some(code) => code == CODE,
            // No code at all means this is the refresh grant.
            None => request.form("refresh_token").is_some(),
        };
        if !identified || !granted {
            return Reply::status(400, r#"{"error":"invalid_grant"}"#);
        }
        return Reply::ok(
            r#"{"access_token": "flow-access", "refresh_token": "flow-refresh",
                "expires_in": 3600, "token_type": "bearer"}"#,
        );
    }
    if path.ends_with("/leagues") {
        return Reply::ok(USER_LEAGUES);
    }
    if path.ends_with("/settings") {
        return match path.contains(AUCTION_KEY) {
            true => Reply::ok(AUCTION_LEAGUE),
            false => Reply::ok(LEAGUE),
        };
    }
    // The keeper flags come off `teams;out=roster`, which is a different
    // resource from the plain team list even though the path starts the same
    // way. Answering the team list to both would leave every roster empty.
    if path.ends_with("/teams;out=roster") {
        return Reply::ok(ROSTERS);
    }
    if path.ends_with("/teams") {
        return Reply::ok(TEAMS);
    }
    if path.ends_with("/draftresults") {
        if path.contains(AUCTION_KEY) {
            return Reply::ok(AUCTION_RESULTS);
        }
        return match advanced.load(Ordering::SeqCst) {
            true => Reply::ok(COMPLETE),
            false => Reply::ok(PARTIAL),
        };
    }
    if path.contains("/players") {
        pool_calls.fetch_add(1, Ordering::SeqCst);
        return Reply::ok(PLAYERS_0);
    }
    Reply::status(404, r#"{"error":{"description":"no such resource"}}"#)
}

/// The Sleeper side: the players dictionary the crosswalk matches against and
/// the projections the board is scored from. Ja'Marr Chase is in it and Bijan
/// Robinson deliberately is not, so one Yahoo player goes unmatched.
const SLEEPER_PLAYERS: &str = r#"{
    "6794": {"full_name": "Ja'Marr Chase", "first_name": "Ja'Marr",
             "last_name": "Chase", "position": "WR", "team": "CIN",
             "fantasy_positions": ["WR"]},
    "4034": {"full_name": "Christian McCaffrey", "position": "RB", "team": "SF",
             "fantasy_positions": ["RB"]},
    "BAL": {"first_name": "Baltimore", "last_name": "Ravens", "position": "DEF",
            "team": "BAL", "fantasy_positions": ["DEF"]}
}"#;

const SLEEPER_PROJECTIONS: &str = r#"[
    {"player_id": "6794", "stats": {"rec": 100.0, "rec_yd": 1400.0, "rec_td": 12.0, "adp_ppr": 2.0},
     "player": {"position": "WR", "team": "CIN"}},
    {"player_id": "4034", "stats": {"rush_yd": 1100.0, "rush_td": 9.0, "rec": 50.0, "adp_ppr": 1.0},
     "player": {"position": "RB", "team": "SF"}},
    {"player_id": "BAL", "stats": {"sack": 45.0, "int": 14.0, "adp_ppr": 120.0},
     "player": {"position": "DEF", "team": "BAL"}}
]"#;

fn sleeper_route(path: &str) -> Option<stub::Reply> {
    let path = path.split('?').next().unwrap_or(path);
    let ok = |body: &str| Some((200u16, body.to_string()));
    match path {
        "/v1/players/nfl" => ok(SLEEPER_PLAYERS),
        "/v1/state/nfl" => ok(r#"{"season": "2026", "week": 1, "display_week": 1}"#),
        "/projections/nfl/2026" => ok(SLEEPER_PROJECTIONS),
        _ if path.starts_with("/projections/nfl/2026/") => ok("[]"),
        _ => None,
    }
}

/// The app on the mock runtime, its Yahoo hosts pointed at `stub` and its
/// secrets pinned to a file in the scratch directory.
pub(crate) struct Session {
    pub(crate) app: tauri::App<tauri::test::MockRuntime>,
    webview: tauri::WebviewWindow<tauri::test::MockRuntime>,
    data_dir: std::path::PathBuf,
    /// Flip it and this session's Yahoo stub serves the finished draft.
    pub(crate) advanced: Arc<AtomicBool>,
    /// How many player-pool pages this session's stub has served. The pool is
    /// 25 rows a page, so it is the one call worth not repeating.
    pub(crate) pool_calls: Arc<AtomicU64>,
    _stub: Stub,
}

pub(crate) fn session(label: &str) -> Session {
    stub::serve(sleeper_route);
    let advanced = Arc::new(AtomicBool::new(false));
    let served = advanced.clone();
    let pool_calls = Arc::new(AtomicU64::new(0));
    let counted = pool_calls.clone();
    let yahoo_stub = yahoo_stub::serve(move |request| yahoo_route(request, &served, &counted));
    let hosts = YahooHosts {
        api_base: format!("{}/fantasy/v2", yahoo_stub.base()),
        login_base: yahoo_stub.base(),
        redirect_uri: "oob".into(),
    };
    let data_dir = stub::scratch_dir(label);
    let engine = Engine::new(data_dir.clone());
    let config = engine.load_config();
    let app = mock_builder()
        .invoke_handler(tauri::generate_handler![
            draft::add_league,
            draft::get_config,
            draft::get_state,
            draft::refresh_picks,
            draft::start_polling,
            draft::stop_polling,
            yahoo::yahoo_status,
            yahoo::yahoo_save_credentials,
            yahoo::yahoo_begin_connect,
            yahoo::yahoo_finish_connect,
            yahoo::yahoo_disconnect,
            yahoo::yahoo_leagues,
            yahoo::yahoo_auction,
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
        yahoo: Arc::new(YahooState::sandboxed(hosts)),
    });
    let webview = WebviewWindowBuilder::new(&app, "main", Default::default())
        .build()
        .expect("the main webview builds");
    Session {
        app,
        webview,
        data_dir,
        advanced,
        pool_calls,
        _stub: yahoo_stub,
    }
}

impl Session {
    pub(crate) fn invoke(&self, cmd: &str, args: Value) -> Result<Value, String> {
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

    pub(crate) fn ok(&self, cmd: &str, args: Value) -> Value {
        self.invoke(cmd, args)
            .unwrap_or_else(|error| panic!("{cmd} failed: {error}"))
    }

    pub(crate) fn err(&self, cmd: &str, args: Value) -> String {
        self.invoke(cmd, args)
            .err()
            .unwrap_or_else(|| panic!("{cmd} was expected to fail"))
    }

    /// Save the credentials and complete the sign-in, which is what every
    /// test below one starts from.
    pub(crate) fn connect(&self) -> Value {
        self.ok(
            "yahoo_save_credentials",
            json!({"clientId": CLIENT_ID, "clientSecret": SECRET}),
        );
        let start = self.ok("yahoo_begin_connect", json!({}));
        let state = start["state"].as_str().expect("a state to echo back");
        self.ok(
            "yahoo_finish_connect",
            json!({"code": CODE, "state": state}),
        )
    }

    pub(crate) fn finish(self) {
        std::fs::remove_dir_all(&self.data_dir).ok();
        drop(self.webview);
        drop(self.app);
    }
}
