pub mod board;
pub mod cache;
pub mod chat;
pub mod chat_cli;
pub mod chat_context;
pub mod commands_chat;
pub mod commands_draft;
pub mod commands_season;
pub mod draft;
pub mod engine;
pub mod headshots;
pub mod keepers;
pub mod mock_league;
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
pub mod season_sources;
pub mod season_trades;
pub mod season_trends_view;
pub mod season_types;
pub mod season_view_feeds;
pub mod season_view_live;
pub mod season_view_market;
pub mod season_view_matchup;
pub mod season_view_standings;
pub mod secrets;
pub mod simulation;
pub mod sleeper;
pub mod sleeper_error;
pub mod state;
pub mod traded_picks;
pub mod valuation;
pub mod view;
pub mod weekly;

use commands_chat::{ask_claude, chat_settings, chat_suggestions, set_api_key, set_chat_provider};
use commands_draft::{
    add_league, export_state, get_config, get_state, record_manual_pick, refresh_data,
    refresh_picks, set_my_username, start_polling, stop_polling, undo_manual_pick,
};
use commands_season::{
    avatar, get_season, headshot, load_season, refresh_season, start_season_polling,
    stop_season_polling,
};
use engine::Engine;
use state::AppState;
use std::sync::atomic::{AtomicBool, AtomicU64};
use std::sync::Arc;
use tauri::Manager;
use tokio::sync::Mutex;

#[cfg_attr(mobile, tauri::mobile_entry_point)]
pub fn run() {
    let builder = tauri::Builder::default().setup(|app| {
        let data_dir = app.path().app_data_dir().expect("no app data dir");
        let engine = Engine::new(data_dir);
        let config = engine.load_config();
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
            chat_settings,
            ask_claude,
            chat_suggestions,
        ])
        .run(tauri::generate_context!())
        .expect("error while running tauri application");
}
