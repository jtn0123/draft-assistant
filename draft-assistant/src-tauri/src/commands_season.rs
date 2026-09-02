//! Tauri commands for the in-season screen.

use crate::engine::{Engine, LoadedLeague};
use crate::headshots::ImageCache;
use crate::poll::{season_tick, SeasonPollMemory};
use crate::season::SeasonView;
use crate::season_engine::{LoadedSeason, SeasonLoader};
use crate::season_history::HistoryStore;
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
        let guard = state.loaded.lock().await;
        // The load above took a few seconds with no lock held. If the user
        // switched leagues in that window, this data belongs to the old one:
        // writing it would file league A's roster snapshot under league B.
        adopt_load(&state.engine, guard.as_ref(), &league.league_id, &mut fresh).await?;
    }
    *state.season.lock().await = Some(fresh);
    season_view_from(&state).await
}

/// Take the Trends snapshot for a finished season load, but only if the league
/// it was loaded for is still the loaded one. Refusing here is what keeps one
/// league's roster snapshot out of another league's history file.
async fn adopt_load(
    engine: &Engine,
    loaded: Option<&LoadedLeague>,
    loaded_for: &str,
    fresh: &mut LoadedSeason,
) -> Result<(), String> {
    let loaded = same_league(loaded, loaded_for)?;
    fresh.history = engine.record_history(loaded, fresh).await;
    Ok(())
}

/// The still-loaded league, if it is the one a slow load was started for.
///
/// Compares the league id rather than counting loads: the id is the thing the
/// result actually has to match, and it needs no extra state on `AppState`.
fn same_league<'a>(
    loaded: Option<&'a LoadedLeague>,
    loaded_for: &str,
) -> Result<&'a LoadedLeague, String> {
    let loaded = loaded.ok_or("the league was closed while this was loading — try again")?;
    if loaded.league.league_id != loaded_for {
        return Err("the league changed while this was loading — try again".to_string());
    }
    Ok(loaded)
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
    let watching = {
        let season = state.season.lock().await;
        let season = season.as_ref().ok_or("season data not loaded")?;
        (season.season, season.week)
    };
    // Fetched with nothing locked: three requests with retries behind them can
    // run for tens of seconds, and everything else that needs the season would
    // be waiting the whole time.
    let fetched = state
        .engine
        .fetch_live(&league_id, watching.0, watching.1)
        .await;
    {
        let mut season = state.season.lock().await;
        let season = season.as_mut().ok_or("season data not loaded")?;
        fetched.apply(season, crate::engine::now_secs())?;
    }
    season_view_from(&state).await
}

/// How many polls to reuse the cached analysis before rebuilding it. At the
/// default 30s interval that is roughly ten minutes.
const ANALYSIS_EVERY: u32 = 20;

/// Poll live scoring every `interval_secs` (default 30). Emits "season-updated"
/// with a fresh SeasonView whenever the totals move, and "season-poll-health"
/// after every attempt so the screen can say when the feed is failing.
///
/// Always succeeds. Starting the poller is not something that can go wrong:
/// asking twice replaces the running loop, and asking before a league is open
/// leaves a loop that picks one up as soon as there is one. So a rejection
/// reaching the screen really does mean live updates are not running.
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
        let mut memory = SeasonPollMemory::new(ANALYSIS_EVERY);
        loop {
            if !polling.load(Ordering::SeqCst)
                || season_generation.load(Ordering::SeqCst) != generation
            {
                break;
            }
            let tick =
                season_tick(&*engine, &loaded_ref, &season_ref, &config_ref, &mut memory).await;
            // Health first: when a refresh fails there is no view to send, and
            // the screen still has to hear that the attempt was made and lost.
            if let Some(health) = &tick.health {
                app.emit("season-poll-health", health).ok();
            }
            if let Some(view) = &tick.view {
                app.emit("season-updated", view).ok();
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

#[cfg(test)]
mod tests {
    use super::*;
    use crate::roster::RosterRules;
    use crate::season_api::Roster;
    use crate::season_history::History;
    use crate::sleeper::{Draft, League};
    use crate::valuation::ReplacementModel;
    use crate::weekly::WeeklyPoints;
    use std::collections::HashMap;
    use std::path::PathBuf;

    fn test_dir(label: &str) -> PathBuf {
        std::env::temp_dir().join(format!(
            "draft-assistant-load-season-{label}-{}-{}",
            std::process::id(),
            std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .unwrap()
                .as_nanos()
        ))
    }

    fn loaded_league(league_id: &str) -> LoadedLeague {
        let league: League = serde_json::from_value(serde_json::json!({
            "league_id": league_id,
            "name": "A League",
            "season": "2025",
            "status": "in_season",
            "total_rosters": 1,
            "roster_positions": ["RB", "BN"],
            "scoring_settings": {"rec": 1.0},
        }))
        .unwrap();
        let draft: Draft = serde_json::from_value(serde_json::json!({
            "draft_id": "draft-1",
            "status": "complete",
            "type": "snake",
            "settings": {"teams": 1, "rounds": 2},
        }))
        .unwrap();
        let roster_rules = RosterRules::new(&league.roster_positions);
        LoadedLeague {
            league,
            draft,
            user_names: HashMap::new(),
            user_avatars: HashMap::new(),
            board: Vec::new(),
            board_index: HashMap::new(),
            replacement_model: ReplacementModel {
                demand: HashMap::new(),
                baseline: HashMap::new(),
            },
            roster_rules,
            api_picks: Vec::new(),
            manual_picks: Vec::new(),
            traded_picks: Vec::new(),
            keeper_pick_nos: Default::default(),
            poll_last_success_at: None,
            poll_consecutive_failures: 0,
            poll_last_error: None,
            players_fetched_at: 0,
            projections_fetched_at: 0,
            weekly_fetched_at: 0,
            warnings: Vec::new(),
            player_meta: HashMap::new(),
            weekly_points: WeeklyPoints::default(),
            second_opinion_loaded_at: None,
        }
    }

    fn season() -> LoadedSeason {
        let roster: Roster = serde_json::from_value(serde_json::json!({
            "roster_id": 1,
            "owner_id": "user-1",
            "players": ["rb1"],
        }))
        .unwrap();
        LoadedSeason {
            week: 2,
            season: 2025,
            rosters: vec![roster],
            matchups: Vec::new(),
            schedule: Vec::new(),
            season_points: HashMap::new(),
            transactions: Vec::new(),
            scores: Vec::new(),
            last_season: Vec::new(),
            history: History::default(),
            fetched_at: 0,
            warnings: Vec::new(),
            sources: Default::default(),
        }
    }

    #[tokio::test]
    async fn a_load_that_finished_after_a_league_switch_is_thrown_away() {
        let dir = test_dir("switched");
        let engine = Engine::new(dir.clone());
        let now_loaded = loaded_league("league-b");
        let mut fresh = season();

        // The load was started for league A; league B is loaded by the time it
        // came back.
        let err = adopt_load(&engine, Some(&now_loaded), "league-a", &mut fresh)
            .await
            .unwrap_err();
        assert!(err.contains("changed"), "unexpected message: {err}");
        assert!(fresh.history.snapshots.is_empty(), "history was recorded");
        assert!(
            !dir.join("history_league-b.json").exists(),
            "league A's season was filed under league B"
        );

        // The same load, still under the league it was started for, records.
        adopt_load(&engine, Some(&now_loaded), "league-b", &mut fresh)
            .await
            .unwrap();
        assert_eq!(fresh.history.snapshots.len(), 1);
        assert!(dir.join("history_league-b.json").exists());

        std::fs::remove_dir_all(dir).unwrap();
    }

    #[tokio::test]
    async fn a_load_that_outlived_its_league_is_thrown_away() {
        let dir = test_dir("closed");
        let engine = Engine::new(dir.clone());
        let mut fresh = season();
        let err = adopt_load(&engine, None, "league-a", &mut fresh)
            .await
            .unwrap_err();
        assert!(err.contains("closed"), "unexpected message: {err}");
        assert!(fresh.history.snapshots.is_empty());
        std::fs::remove_dir_all(dir).unwrap();
    }
}
