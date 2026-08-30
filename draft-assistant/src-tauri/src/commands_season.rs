//! Tauri commands for the in-season screen.

use crate::season::{build_season_view_cached, SeasonAnalysis, SeasonView};
use crate::state::{season_view_from, AppState};
use std::sync::atomic::Ordering;
use tauri::{Emitter, State};

/// Fetch the in-season picture for the active league. Idempotent: a second
/// call reuses cached data unless `force` is set.
#[tauri::command]
pub async fn load_season(
    state: State<'_, AppState>,
    force: Option<bool>,
) -> Result<SeasonView, String> {
    let force = force.unwrap_or(false);
    let league = {
        let loaded = state.loaded.lock().await;
        loaded.as_ref().ok_or("no league loaded")?.league.clone()
    };
    let my_user_id = state.config.lock().await.my_user_id.clone();
    let mut fresh = state
        .engine
        .load_season(&league, my_user_id.as_deref(), force)
        .await?;
    {
        let loaded = state.loaded.lock().await;
        if let Some(loaded) = loaded.as_ref() {
            fresh.history = state.engine.record_history(loaded, &fresh);
        }
    }
    *state.season.lock().await = Some(fresh);
    season_view_from(&state).await
}

/// A player's photo as a data URL (cached on disk), or null when there is none.
#[tauri::command]
pub async fn headshot(
    state: State<'_, AppState>,
    player_id: String,
) -> Result<Option<String>, String> {
    state.engine.headshot(&player_id).await
}

/// A manager's team picture as a data URL (cached on disk), or null.
#[tauri::command]
pub async fn avatar(
    state: State<'_, AppState>,
    reference: String,
    full: bool,
) -> Result<Option<String>, String> {
    state.engine.avatar(&reference, full).await
}

/// The current season view, without refetching.
#[tauri::command]
pub async fn get_season(state: State<'_, AppState>) -> Result<SeasonView, String> {
    season_view_from(&state).await
}

/// Re-pull the fast-moving slice (this week's scoring and the NFL scoreboard).
#[tauri::command]
pub async fn refresh_season(state: State<'_, AppState>) -> Result<SeasonView, String> {
    let league_id = {
        let loaded = state.loaded.lock().await;
        loaded
            .as_ref()
            .ok_or("no league loaded")?
            .league
            .league_id
            .clone()
    };
    {
        let mut season = state.season.lock().await;
        let season = season.as_mut().ok_or("season data not loaded")?;
        state.engine.refresh_live(season, &league_id).await?;
    }
    season_view_from(&state).await
}

/// How many polls to reuse the cached analysis before rebuilding it. At the
/// default 30s interval that is roughly ten minutes.
const ANALYSIS_EVERY: u32 = 20;

/// Poll live scoring every `interval_secs` (default 30). Emits "season-updated"
/// with a fresh SeasonView whenever the totals move.
#[tauri::command]
pub async fn start_season_polling(
    app: tauri::AppHandle,
    state: State<'_, AppState>,
    interval_secs: Option<u64>,
) -> Result<(), String> {
    let interval = interval_secs.unwrap_or(30).clamp(10, 300);
    let generation = state.season_generation.fetch_add(1, Ordering::SeqCst) + 1;
    state.season_polling.store(true, Ordering::SeqCst);

    let engine = state.engine.clone();
    let loaded_ref = state.loaded.clone();
    let season_ref = state.season.clone();
    let config_ref = state.config.clone();
    let polling = state.season_polling.clone();
    let season_generation = state.season_generation.clone();

    tauri::async_runtime::spawn(async move {
        let mut last_totals: Option<(u64, u64)> = None;
        // Computed on the first tick and reused after: the poll only refreshes
        // live scoring, which cannot move playoff odds, waivers or trades.
        // Rebuilt every ANALYSIS_EVERY ticks so a waiver claim or a trade
        // elsewhere in the league still works its way in.
        let mut analysis: Option<SeasonAnalysis> = None;
        let mut ticks: u32 = 0;
        loop {
            if !polling.load(Ordering::SeqCst)
                || season_generation.load(Ordering::SeqCst) != generation
            {
                break;
            }
            let league_id = {
                let loaded = loaded_ref.lock().await;
                loaded.as_ref().map(|l| l.league.league_id.clone())
            };
            if let Some(league_id) = league_id {
                let refreshed = {
                    let mut season = season_ref.lock().await;
                    match season.as_mut() {
                        Some(season) => engine.refresh_live(season, &league_id).await.is_ok(),
                        None => false,
                    }
                };
                if refreshed {
                    let loaded = loaded_ref.lock().await;
                    let season = season_ref.lock().await;
                    let config = config_ref.lock().await;
                    if let (Some(loaded), Some(season)) = (loaded.as_ref(), season.as_ref()) {
                        let view = build_season_view_cached(
                            loaded,
                            season,
                            config.my_user_id.as_deref(),
                            analysis.as_ref(),
                        );
                        if analysis.is_none() {
                            analysis = Some(SeasonAnalysis::of(&view));
                        }
                        ticks += 1;
                        if ticks % ANALYSIS_EVERY == 0 {
                            analysis = None;
                        }
                        // Emit only when a score actually moved: this view is
                        // large and the panel re-renders on every event.
                        let totals = (
                            (view.live.totals.my_live_points * 100.0) as u64,
                            (view.live.totals.opp_live_points * 100.0) as u64,
                        );
                        if last_totals != Some(totals) {
                            last_totals = Some(totals);
                            app.emit("season-updated", &view).ok();
                        }
                    }
                }
            }
            tokio::time::sleep(std::time::Duration::from_secs(interval)).await;
        }
    });
    Ok(())
}

#[tauri::command]
pub async fn stop_season_polling(state: State<'_, AppState>) -> Result<(), String> {
    state.season_polling.store(false, Ordering::SeqCst);
    Ok(())
}
