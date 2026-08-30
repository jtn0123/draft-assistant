//! Shared application state and the view builders every command goes through.

use crate::engine::{AppConfig, Engine, LoadedLeague};
use crate::season::{build_season_view, SeasonView};
use crate::season_engine::LoadedSeason;
use crate::view::{build_view, DraftView};
use std::sync::atomic::{AtomicBool, AtomicU64};
use std::sync::Arc;
use tauri::State;
use tokio::sync::Mutex;

pub struct AppState {
    pub engine: Arc<Engine>,
    pub loaded: Arc<Mutex<Option<LoadedLeague>>>,
    pub season: Arc<Mutex<Option<LoadedSeason>>>,
    pub config: Arc<Mutex<AppConfig>>,
    pub polling: Arc<AtomicBool>,
    pub poll_generation: Arc<AtomicU64>,
    pub season_polling: Arc<AtomicBool>,
    pub season_generation: Arc<AtomicU64>,
}

pub fn view_from(loaded: &LoadedLeague, config: &AppConfig) -> DraftView {
    build_view(loaded, config)
}

/// Pull the Sleeper ID out of whatever the user pasted — a bare ID or a full
/// URL like https://sleeper.com/draft/nfl/139888...?ftue=commish.
/// Build the season view from whatever is already loaded.
pub async fn season_view_from(state: &State<'_, AppState>) -> Result<SeasonView, String> {
    let loaded = state.loaded.lock().await;
    let loaded = loaded.as_ref().ok_or("no league loaded")?;
    let season = state.season.lock().await;
    let season = season.as_ref().ok_or("season data not loaded")?;
    let config = state.config.lock().await;
    Ok(build_season_view(
        loaded,
        season,
        config.my_user_id.as_deref(),
    ))
}
