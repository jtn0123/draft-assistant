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
    /// The last view the season screen asked for. Chat answers from this
    /// rather than building its own, which is what keeps a question from
    /// stalling both pollers for the length of a full rebuild.
    pub last_season_view: Arc<Mutex<Option<Arc<SeasonView>>>>,
}

pub fn view_from(loaded: &LoadedLeague, config: &AppConfig) -> DraftView {
    build_view(loaded, config)
}

/// Pull the Sleeper ID out of whatever the user pasted — a bare ID or a full
/// URL like https://sleeper.com/draft/nfl/139888...?ftue=commish.
/// Build the season view from whatever is already loaded.
pub async fn season_view_from(state: &State<'_, AppState>) -> Result<SeasonView, String> {
    let view = {
        let loaded = state.loaded.lock().await;
        let loaded = loaded.as_ref().ok_or("no league loaded")?;
        let season = state.season.lock().await;
        let season = season.as_ref().ok_or("season data not loaded")?;
        let config = state.config.lock().await;
        Arc::new(build_season_view(
            loaded,
            season,
            config.my_user_id.as_deref(),
        ))
    };
    // Remember it for the chat panel, which would otherwise pay for the whole
    // build again on every question.
    *state.last_season_view.lock().await = Some(view.clone());
    Ok((*view).clone())
}

/// Everything [`build_season_view`] reads, copied out of shared state so the
/// build itself can run with nothing locked.
pub struct SeasonInputs {
    league: LoadedLeague,
    season: LoadedSeason,
    my_user_id: Option<String>,
}

/// Copy the build's inputs, taking the three mutexes in the order the rest of
/// the app takes them (loaded, then season, then config) and releasing every
/// one of them before returning.
pub async fn season_inputs(
    loaded: &Mutex<Option<LoadedLeague>>,
    season: &Mutex<Option<LoadedSeason>>,
    config: &Mutex<AppConfig>,
) -> Result<SeasonInputs, String> {
    let loaded = loaded.lock().await;
    let league = loaded.as_ref().ok_or("no league loaded")?.clone();
    let season = season.lock().await;
    let season = season.as_ref().ok_or("season data not loaded")?.clone();
    let config = config.lock().await;
    Ok(SeasonInputs {
        league,
        season,
        my_user_id: config.my_user_id.clone(),
    })
}

/// Build a season view on the blocking pool. The thousand-odd lineup solves
/// and the playoff simulation are plain CPU work, and running them on a
/// runtime thread stops every other task — including both pollers.
pub async fn build_season_off_thread(inputs: SeasonInputs) -> Result<SeasonView, String> {
    tokio::task::spawn_blocking(move || {
        build_season_view(&inputs.league, &inputs.season, inputs.my_user_id.as_deref())
    })
    .await
    .map_err(|e| format!("could not put the season summary together: {e}"))
}

/// The season view a chat question should be answered from.
///
/// The season screen builds one every time it is opened or refreshed, and
/// nothing the chat summary reads out of it moves with live scoring, so that
/// view is reused as it stands. Only when there is none — or when it belongs
/// to a league the user has since switched away from — does chat build its
/// own, and then it copies the inputs, drops every guard, and hands the work
/// to a blocking thread.
pub async fn season_view_for_chat(
    loaded: &Mutex<Option<LoadedLeague>>,
    season: &Mutex<Option<LoadedSeason>>,
    config: &Mutex<AppConfig>,
    last: &Mutex<Option<Arc<SeasonView>>>,
) -> Result<Arc<SeasonView>, String> {
    let league_id = {
        let guard = loaded.lock().await;
        guard
            .as_ref()
            .ok_or("no league loaded")?
            .league
            .league_id
            .clone()
    };
    if season.lock().await.is_none() {
        return Err("season data not loaded".to_string());
    }
    let cached = last.lock().await.clone();
    if let Some(view) = cached.filter(|v| v.league.league_id == league_id) {
        return Ok(view);
    }
    let inputs = season_inputs(loaded, season, config).await?;
    let view = Arc::new(build_season_off_thread(inputs).await?);
    *last.lock().await = Some(view.clone());
    Ok(view)
}
