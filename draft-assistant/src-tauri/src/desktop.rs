//! The Tauri desktop shell: the command surface and app bootstrap. Every
//! command forwards to [`AppCore`], which holds the state and the logic and
//! is tested without Tauri (`tests/app_core.rs`); this file only adapts
//! IPC types — `State`, `Channel`, window events — to it.
//!
//! Split out of `lib.rs` so the domain library compiles without Tauri at all
//! (`--no-default-features`), which is what the fuzz targets link against.

use crate::app::{AppCore, PollEvent};
use crate::chat::{ChatOptions, ChatReply, ChatTurn};
use crate::engine::{AppConfig, Engine};
use crate::view::DraftView;
use std::sync::Arc;
use std::time::Duration;
use tauri::ipc::Channel;
use tauri::{Emitter, Manager, State};

struct AppState {
    core: Arc<AppCore>,
}

#[tauri::command]
async fn add_league(
    state: State<'_, AppState>,
    league_id: String,
    force: Option<bool>,
) -> Result<DraftView, String> {
    state
        .core
        .add_league(&league_id, force.unwrap_or(false))
        .await
}

#[tauri::command]
async fn set_my_username(state: State<'_, AppState>, username: String) -> Result<String, String> {
    state.core.set_my_username(&username).await
}

#[tauri::command]
async fn get_config(state: State<'_, AppState>) -> Result<AppConfig, String> {
    Ok(state.core.get_config().await)
}

#[tauri::command]
async fn get_state(state: State<'_, AppState>) -> Result<DraftView, String> {
    state.core.get_state().await
}

#[tauri::command]
async fn refresh_picks(state: State<'_, AppState>) -> Result<DraftView, String> {
    state.core.refresh_picks().await
}

#[tauri::command]
async fn refresh_data(state: State<'_, AppState>) -> Result<DraftView, String> {
    state.core.refresh_data().await
}

#[tauri::command]
async fn record_manual_pick(
    state: State<'_, AppState>,
    player_id: String,
) -> Result<DraftView, String> {
    state.core.record_manual_pick(player_id).await
}

#[tauri::command]
async fn undo_manual_pick(state: State<'_, AppState>) -> Result<DraftView, String> {
    state.core.undo_manual_pick().await
}

#[tauri::command]
async fn export_state(state: State<'_, AppState>) -> Result<String, String> {
    state.core.export_state().await
}

/// Ask Claude; the answer is streamed back over `on_text` as it is written
/// and returned whole at the end.
#[tauri::command]
async fn chat(
    state: State<'_, AppState>,
    question: String,
    history: Vec<ChatTurn>,
    options: ChatOptions,
    on_text: Channel<String>,
) -> Result<ChatReply, String> {
    let mut send = |text: &str| {
        // A closed channel means the panel went away; the answer still
        // completes and is returned whole.
        let _ = on_text.send(text.to_string());
    };
    state
        .core
        .ask(&question, &history, &options, &mut send)
        .await
}

#[tauri::command]
async fn chat_compact(
    state: State<'_, AppState>,
    history: Vec<ChatTurn>,
    options: ChatOptions,
) -> Result<ChatReply, String> {
    state.core.compact(&history, &options).await
}

/// Start polling Sleeper every `interval_secs` (default 3). Emits
/// "poll-health" after every poll and "draft-updated" with the fresh view
/// whenever the feed changed.
#[tauri::command]
async fn start_polling(
    app: tauri::AppHandle,
    state: State<'_, AppState>,
    interval_secs: Option<u64>,
) -> Result<(), String> {
    let interval = Duration::from_secs(interval_secs.unwrap_or(3).clamp(2, 60));
    let generation = state.core.begin_polling();
    let core = state.core.clone();
    tauri::async_runtime::spawn(async move {
        let emit = move |event: PollEvent| match event {
            PollEvent::Health(health) => {
                app.emit("poll-health", &health).ok();
            }
            PollEvent::View(view) => {
                app.emit("draft-updated", &view).ok();
            }
        };
        core.poll_loop(interval, generation, &emit).await;
    });
    Ok(())
}

#[tauri::command]
async fn stop_polling(state: State<'_, AppState>) -> Result<(), String> {
    state.core.stop_polling();
    Ok(())
}

#[cfg_attr(mobile, tauri::mobile_entry_point)]
pub fn run() {
    tauri::Builder::default()
        .plugin(tauri_plugin_opener::init())
        .setup(|app| {
            let data_dir = app.path().app_data_dir().expect("no app data dir");
            let engine = Engine::new(data_dir)?;
            app.manage(AppState {
                core: Arc::new(AppCore::new(engine)),
            });
            Ok(())
        })
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
            chat,
            chat_compact,
            start_polling,
            stop_polling,
        ])
        .run(tauri::generate_context!())
        .expect("error while running tauri application");
}
