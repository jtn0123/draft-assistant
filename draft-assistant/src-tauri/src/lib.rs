pub mod applog;
pub mod backtest;
pub mod board;
pub mod cache;
pub mod chat;
pub mod chat_cli;
pub mod chat_client;
pub mod chat_context;
pub mod chat_copy;
/// Hand-built views for the chat context tests.
#[cfg(test)]
pub mod chat_fixtures;
pub mod chat_rules;
pub mod commands_chat;
pub mod commands_companion;
pub mod commands_draft;
pub mod commands_season;
pub mod commands_second_opinion;
pub mod commands_yahoo;
pub mod companion;
pub mod draft;
pub mod engine;
pub mod engine_assemble;
pub mod engine_yahoo;
pub mod engine_yahoo_pool;
pub mod headshots;
pub mod keepers;
pub mod league_ref;
pub mod leagues;
pub mod mock_league;
pub mod pick_value;
pub mod picks;
pub mod poll;
pub mod projections;
pub mod recommend;
pub mod roster;
pub mod scoring;
pub mod season;
pub mod season_activity;
pub mod season_api;
pub mod season_calls;
pub mod season_deals;
pub mod season_engine;
pub mod season_history;
pub mod season_injury;
pub mod season_lineup;
pub mod season_live;
pub mod season_lookup;
pub mod season_moves;
pub mod season_odds;
pub mod season_refresh;
pub mod season_sources;
pub mod season_spread;
pub mod season_trades;
pub mod season_trends_view;
pub mod season_types;
pub mod season_view_feeds;
pub mod season_view_live;
pub mod season_view_market;
pub mod season_view_matchup;
pub mod season_view_standings;
pub mod second_opinion;
pub mod secrets;
pub mod shared_chat;
pub mod simulation;
pub mod sleeper;
pub mod sleeper_error;
pub mod sleeper_host;
pub mod sleeper_users;
pub mod state;
pub mod traded_picks;
pub mod valuation;
pub mod view;
pub mod view_signals;
pub mod view_types;
pub mod weekly;
pub mod yahoo;
pub mod yahoo_crosswalk;
pub mod yahoo_map;
pub mod yahoo_oauth;
pub mod yahoo_parse;
pub mod yahoo_pool;
pub mod yahoo_retry;
pub mod yahoo_secrets;
pub mod yahoo_types;

use commands_chat::{
    ask_claude, chat_settings, chat_suggestions, set_api_key, set_chat_budget, set_chat_provider,
};
use commands_companion::{
    companion_disable, companion_enable, companion_revoke, companion_status, set_device_name,
    shared_chat_get, shared_chat_send,
};
use commands_draft::{
    add_league, clear_keepers, export_state, get_config, get_state, record_manual_pick,
    refresh_data, refresh_picks, set_my_username, start_polling, stop_polling, undo_manual_pick,
};
use commands_season::{
    avatar, get_season, headshot, load_season, refresh_season, start_season_polling,
    stop_season_polling,
};
use commands_second_opinion::import_second_opinion;
use commands_yahoo::{
    yahoo_auction, yahoo_begin_connect, yahoo_disconnect, yahoo_finish_connect, yahoo_leagues,
    yahoo_save_credentials, yahoo_status,
};
use companion::CompanionServer;
use engine::Engine;
use leagues::{remove_league, sleeper_leagues};
use state::{AppState, YahooState};
use std::sync::atomic::{AtomicBool, AtomicU64};
use std::sync::Arc;
use tauri::{Emitter, Manager};
use tokio::sync::Mutex;

#[cfg_attr(mobile, tauri::mobile_entry_point)]
pub fn run() {
    let builder = tauri::Builder::default()
        // Opens the native file picker for Settings -> "Import projections
        // CSV...". Its commands are not in capabilities/default.json, so the
        // webview cannot open a picker of its own.
        .plugin(tauri_plugin_dialog::init())
        .setup(|app| {
            let data_dir = app.path().app_data_dir().expect("no app data dir");
            // First point in the process where there is anywhere to write:
            // until this runs, `applog::warn` falls back to a stderr that a
            // double-clicked .app does not have.
            applog::init(data_dir.clone());
            let engine = Engine::new(data_dir.clone());
            let config = engine.load_config();
            // The name this Mac introduces itself by, and the port its phone
            // server last used. Read before the config is handed over.
            let host_name = config
                .device_name
                .clone()
                .unwrap_or_else(commands_companion::default_host_name);
            let state = AppState {
                engine: Arc::new(engine),
                loaded: Arc::new(Mutex::new(None)),
                season: Arc::new(Mutex::new(None)),
                config: Arc::new(Mutex::new(config)),
                polling: Arc::new(AtomicBool::new(false)),
                poll_generation: Arc::new(AtomicU64::new(0)),
                season_polling: Arc::new(AtomicBool::new(false)),
                season_generation: Arc::new(AtomicU64::new(0)),
                last_season_view: Arc::new(Mutex::new(None)),
                yahoo: Arc::new(YahooState::default()),
            };
            // The companion server is built at startup but not started: it is
            // off until the user turns it on in Settings. What it holds before
            // then is the pairing code, the shared chat threads, and a handle
            // onto the same state every command works through.
            let companion = Arc::new(CompanionServer::new(host_name, data_dir)?);
            let handle = app.handle().clone();
            companion.attach(
                Arc::new(state.share()),
                Arc::new(move |kind: &str, payload: serde_json::Value| {
                    handle.emit(kind, payload).ok();
                }),
            );
            app.manage(state);
            app.manage(companion);
            Ok(())
        });

    // The only thing that puts a WebDriver server -- a full remote-control
    // surface -- inside this app, and it is compiled out unless the `wdio`
    // cargo feature is on. That feature is off by default and is set by
    // nothing but `npm run test:e2e`; `build.rs` gates the matching
    // capability on the same flag. A release bundle therefore does not
    // contain these plugins, not even dormant.
    #[cfg(feature = "wdio")]
    let builder = builder
        .plugin(tauri_plugin_wdio::init())
        .plugin(tauri_plugin_wdio_webdriver::init());

    builder
        .invoke_handler(tauri::generate_handler![
            add_league,
            set_my_username,
            get_config,
            get_state,
            refresh_picks,
            refresh_data,
            record_manual_pick,
            undo_manual_pick,
            clear_keepers,
            export_state,
            start_polling,
            stop_polling,
            load_season,
            get_season,
            refresh_season,
            start_season_polling,
            stop_season_polling,
            headshot,
            avatar,
            set_api_key,
            set_chat_provider,
            set_chat_budget,
            chat_settings,
            ask_claude,
            chat_suggestions,
            sleeper_leagues,
            remove_league,
            import_second_opinion,
            yahoo_status,
            yahoo_save_credentials,
            yahoo_begin_connect,
            yahoo_finish_connect,
            yahoo_disconnect,
            yahoo_leagues,
            yahoo_auction,
            companion_status,
            companion_enable,
            companion_disable,
            companion_revoke,
            set_device_name,
            shared_chat_get,
            shared_chat_send,
        ])
        .run(tauri::generate_context!())
        .expect("error while running tauri application");
}
