pub mod board;
pub mod draft;
pub mod engine;
pub mod recommend;
pub mod view;
pub mod scoring;
pub mod sleeper;
pub mod valuation;

use engine::{AppConfig, DraftView, Engine, LoadedLeague, StoredLeague};
use sleeper::Pick;
use std::sync::atomic::{AtomicBool, AtomicU64, Ordering};
use std::sync::Arc;
use tauri::{Emitter, Manager, State};
use tokio::sync::Mutex;

struct AppState {
    engine: Arc<Engine>,
    loaded: Arc<Mutex<Option<LoadedLeague>>>,
    config: Arc<Mutex<AppConfig>>,
    polling: Arc<AtomicBool>,
    poll_generation: Arc<AtomicU64>,
}

fn view_from(loaded: &LoadedLeague, config: &AppConfig) -> DraftView {
    engine::build_view(loaded, config)
}

/// Pull the Sleeper ID out of whatever the user pasted — a bare ID or a full
/// URL like https://sleeper.com/draft/nfl/139888...?ftue=commish.
fn extract_id(input: &str) -> String {
    input
        .split(|c: char| !c.is_ascii_digit())
        .max_by_key(|run| run.len())
        .filter(|run| run.len() >= 15)
        .unwrap_or(input.trim())
        .to_string()
}

/// Add (or re-sync) a league by ID, make it active, and build its board.
/// Also accepts a bare draft ID (mock drafts) or a pasted sleeper.com URL.
#[tauri::command]
async fn add_league(
    state: State<'_, AppState>,
    league_id: String,
    force: Option<bool>,
) -> Result<DraftView, String> {
    let force = force.unwrap_or(false);
    let league_id = extract_id(&league_id);
    let new_loaded = state.engine.load_any(&league_id, force).await?;
    let mut config = state.config.lock().await;
    if !config.leagues.iter().any(|l| l.league_id == league_id) {
        config.leagues.push(StoredLeague {
            league_id: league_id.clone(),
            name: new_loaded.league.name.clone(),
            season: new_loaded.league.season.clone(),
        });
    }
    config.active_league_id = Some(league_id);
    state.engine.save_config(&config);
    let view = view_from(&new_loaded, &config);
    *state.loaded.lock().await = Some(new_loaded);
    Ok(view)
}

/// Identify the user by Sleeper username so "my team" resolves per league.
#[tauri::command]
async fn set_my_username(state: State<'_, AppState>, username: String) -> Result<String, String> {
    #[derive(serde::Deserialize)]
    struct User {
        user_id: String,
    }
    let url = format!("https://api.sleeper.app/v1/user/{username}");
    let resp = reqwest::get(&url).await.map_err(|e| e.to_string())?;
    let user: Option<User> = resp.json().await.map_err(|e| e.to_string())?;
    let user = user.ok_or_else(|| format!("Sleeper user '{username}' not found"))?;
    let mut config = state.config.lock().await;
    config.my_user_id = Some(user.user_id.clone());
    state.engine.save_config(&config);
    Ok(user.user_id)
}

#[tauri::command]
async fn get_config(state: State<'_, AppState>) -> Result<AppConfig, String> {
    Ok(state.config.lock().await.clone())
}

/// The one call: full current draft state. This is the UI's data source AND
/// the AI-readable dump.
#[tauri::command]
async fn get_state(state: State<'_, AppState>) -> Result<DraftView, String> {
    let loaded = state.loaded.lock().await;
    let loaded = loaded.as_ref().ok_or("no league loaded")?;
    let config = state.config.lock().await;
    Ok(view_from(loaded, &config))
}

/// Re-poll picks once, right now.
#[tauri::command]
async fn refresh_picks(state: State<'_, AppState>) -> Result<DraftView, String> {
    let mut loaded = state.loaded.lock().await;
    let loaded = loaded.as_mut().ok_or("no league loaded")?;
    let draft_id = loaded.draft.draft_id.clone();
    loaded.api_picks = state.engine.client.picks(&draft_id).await?;
    // Also refresh draft status/order — it flips to "drafting" at start time.
    if let Ok(draft) = state.engine.client.draft(&draft_id).await {
        loaded.draft = draft;
    }
    let config = state.config.lock().await;
    Ok(view_from(loaded, &config))
}

/// Full data refresh (players + projections + board rebuild).
#[tauri::command]
async fn refresh_data(state: State<'_, AppState>) -> Result<DraftView, String> {
    let league_id = {
        let config = state.config.lock().await;
        config.active_league_id.clone().ok_or("no active league")?
    };
    let new_loaded = state.engine.load_any(&league_id, true).await?;
    let config = state.config.lock().await;
    let view = view_from(&new_loaded, &config);
    *state.loaded.lock().await = Some(new_loaded);
    Ok(view)
}

/// Manual pick fallback for API lag or an offline draft. Marks the given
/// player as taken at the current pick.
#[tauri::command]
async fn record_manual_pick(
    state: State<'_, AppState>,
    player_id: String,
) -> Result<DraftView, String> {
    let mut loaded = state.loaded.lock().await;
    let loaded = loaded.as_mut().ok_or("no league loaded")?;
    let teams = loaded.draft.settings.teams;
    let picks = engine::merged_picks(&loaded.api_picks, &loaded.manual_picks);
    if picks.iter().any(|p| p.player_id == player_id) {
        return Err("player already drafted".into());
    }
    let pick_no = picks.len() as u32 + 1;
    if pick_no > teams * loaded.draft.settings.rounds {
        return Err("draft is complete".into());
    }
    loaded.manual_picks.push(Pick {
        round: (pick_no - 1) / teams + 1,
        pick_no,
        draft_slot: draft::slot_for_pick(pick_no, teams),
        player_id,
        picked_by: None,
        metadata: None,
    });
    let config = state.config.lock().await;
    Ok(view_from(loaded, &config))
}

#[tauri::command]
async fn undo_manual_pick(state: State<'_, AppState>) -> Result<DraftView, String> {
    let mut loaded = state.loaded.lock().await;
    let loaded = loaded.as_mut().ok_or("no league loaded")?;
    if loaded.manual_picks.pop().is_none() {
        return Err("no manual picks to undo (API picks cannot be undone locally)".into());
    }
    let config = state.config.lock().await;
    Ok(view_from(loaded, &config))
}

/// Export the full AI-readable state to a JSON file; returns the path.
#[tauri::command]
async fn export_state(state: State<'_, AppState>) -> Result<String, String> {
    let loaded = state.loaded.lock().await;
    let loaded = loaded.as_ref().ok_or("no league loaded")?;
    let config = state.config.lock().await;
    let view = view_from(loaded, &config);
    let path = state.engine.data_dir.join("draft-state.json");
    let json = serde_json::to_string_pretty(&view).map_err(|e| e.to_string())?;
    std::fs::write(&path, json).map_err(|e| e.to_string())?;
    Ok(path.to_string_lossy().to_string())
}

/// Start polling Sleeper picks every `interval_secs` (default 3). Emits a
/// "draft-updated" event with the fresh DraftView whenever anything changed.
#[tauri::command]
async fn start_polling(
    app: tauri::AppHandle,
    state: State<'_, AppState>,
    interval_secs: Option<u64>,
) -> Result<(), String> {
    let interval = interval_secs.unwrap_or(3).clamp(2, 60);
    let generation = state.poll_generation.fetch_add(1, Ordering::SeqCst) + 1;
    state.polling.store(true, Ordering::SeqCst);

    let engine = state.engine.clone();
    let loaded_ref = state.loaded.clone();
    let config_ref = state.config.clone();
    let polling = state.polling.clone();
    let poll_generation = state.poll_generation.clone();

    tauri::async_runtime::spawn(async move {
        let mut last_count: Option<usize> = None;
        let mut last_status = String::new();
        loop {
            if !polling.load(Ordering::SeqCst)
                || poll_generation.load(Ordering::SeqCst) != generation
            {
                break;
            }
            let draft_id = {
                let loaded = loaded_ref.lock().await;
                loaded.as_ref().map(|l| l.draft.draft_id.clone())
            };
            if let Some(draft_id) = draft_id {
                let picks = engine.client.picks(&draft_id).await;
                let draft = engine.client.draft(&draft_id).await;
                let mut changed = false;
                {
                    let mut loaded = loaded_ref.lock().await;
                    if let Some(loaded) = loaded.as_mut() {
                        if let Ok(picks) = picks {
                            if last_count != Some(picks.len()) {
                                last_count = Some(picks.len());
                                changed = true;
                            }
                            loaded.api_picks = picks;
                        }
                        if let Ok(draft) = draft {
                            if draft.status != last_status {
                                last_status = draft.status.clone();
                                changed = true;
                            }
                            loaded.draft = draft;
                        }
                    }
                }
                if changed {
                    let loaded = loaded_ref.lock().await;
                    let config = config_ref.lock().await;
                    if let Some(loaded) = loaded.as_ref() {
                        let view = view_from(loaded, &config);
                        app.emit("draft-updated", &view).ok();
                    }
                }
            }
            tokio::time::sleep(std::time::Duration::from_secs(interval)).await;
        }
    });
    Ok(())
}

#[tauri::command]
async fn stop_polling(state: State<'_, AppState>) -> Result<(), String> {
    state.polling.store(false, Ordering::SeqCst);
    Ok(())
}

#[cfg_attr(mobile, tauri::mobile_entry_point)]
pub fn run() {
    tauri::Builder::default()
        .plugin(tauri_plugin_opener::init())
        .setup(|app| {
            let data_dir = app
                .path()
                .app_data_dir()
                .expect("no app data dir");
            let engine = Engine::new(data_dir);
            let config = engine.load_config();
            app.manage(AppState {
                engine: Arc::new(engine),
                loaded: Arc::new(Mutex::new(None)),
                config: Arc::new(Mutex::new(config)),
                polling: Arc::new(AtomicBool::new(false)),
                poll_generation: Arc::new(AtomicU64::new(0)),
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
            start_polling,
            stop_polling,
        ])
        .run(tauri::generate_context!())
        .expect("error while running tauri application");
}
