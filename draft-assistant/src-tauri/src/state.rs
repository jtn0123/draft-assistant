//! Shared application state and the view builders every command goes through.

use crate::engine::{now_secs, AppConfig, Engine, LoadedLeague};
use crate::season::{build_season_view_cached, SeasonAnalysis, SeasonView};
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
    pub last_season_view: Arc<Mutex<Option<CachedSeasonView>>>,
}

/// A season view kept for the chat panel, with the moment it was built.
///
/// The time is what stops a question asked an hour later being answered from
/// a view of an hour-old week — waivers cleared, a trade went through, half
/// the games finished — while the season screen sat untouched in another tab.
#[derive(Clone)]
pub struct CachedSeasonView {
    pub view: Arc<SeasonView>,
    built_at: u64,
}

impl CachedSeasonView {
    pub fn new(view: Arc<SeasonView>) -> Self {
        Self {
            view,
            built_at: now_secs(),
        }
    }

    /// Whether this is still worth answering from.
    fn usable_for(&self, league_id: &str, now: u64) -> bool {
        cache_is_usable(&self.view.league.league_id, self.built_at, league_id, now)
    }
}

/// The right league, and young enough that the week has not moved on under it.
///
/// The age is what was missing: the season screen builds a view when it is
/// opened, and a question asked from another tab an hour later was answered
/// off that same view — waivers cleared, a trade done, half the games over.
fn cache_is_usable(cached_league: &str, built_at: u64, want: &str, now: u64) -> bool {
    cached_league == want && now.saturating_sub(built_at) < CACHED_VIEW_MAX_AGE_SECS
}

/// How long a built season view is answered from before chat builds its own.
const CACHED_VIEW_MAX_AGE_SECS: u64 = 600;

pub fn view_from(loaded: &LoadedLeague, config: &AppConfig) -> DraftView {
    build_view(loaded, config)
}

/// Pull the Sleeper ID out of whatever the user pasted — a bare ID or a full
/// URL like https://sleeper.com/draft/nfl/139888...?ftue=commish.
/// Build the season view from whatever is already loaded.
pub async fn season_view_from(state: &State<'_, AppState>) -> Result<SeasonView, String> {
    // The inputs are copied out and every guard dropped before the build: it
    // is seconds of lineup solving and playoff simulation, and running it with
    // the three mutexes held on a runtime thread stopped both pollers and
    // every other command for the whole of it.
    let inputs = season_inputs(&state.loaded, &state.season, &state.config).await?;
    let view = Arc::new(build_season_off_thread(inputs).await?);
    // Remember it for the chat panel, which would otherwise pay for the whole
    // build again on every question.
    *state.last_season_view.lock().await = Some(CachedSeasonView::new(view.clone()));
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
    build_season_cached_off_thread(inputs, None).await
}

/// The same, reusing the expensive half a poll tick already computed.
pub async fn build_season_cached_off_thread(
    inputs: SeasonInputs,
    cached: Option<SeasonAnalysis>,
) -> Result<SeasonView, String> {
    tokio::task::spawn_blocking(move || {
        build_season_view_cached(
            &inputs.league,
            &inputs.season,
            inputs.my_user_id.as_deref(),
            cached.as_ref(),
        )
    })
    .await
    .map_err(|e| format!("could not put the season summary together: {e}"))
}

/// The season view a chat question should be answered from.
///
/// The season screen builds one every time it is opened or refreshed, and
/// nothing the chat summary reads out of it moves with live scoring, so that
/// view is reused as it stands. Only when there is none — or when it belongs
/// to a league the user has since switched away from, or was built long enough
/// ago that the week has moved on under it — does chat build its own, and then
/// it copies the inputs, drops every guard, and hands the work to a blocking
/// thread.
pub async fn season_view_for_chat(
    loaded: &Mutex<Option<LoadedLeague>>,
    season: &Mutex<Option<LoadedSeason>>,
    config: &Mutex<AppConfig>,
    last: &Mutex<Option<CachedSeasonView>>,
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
    if let Some(cached) = cached.filter(|c| c.usable_for(&league_id, now_secs())) {
        return Ok(cached.view);
    }
    let inputs = season_inputs(loaded, season, config).await?;
    let view = Arc::new(build_season_off_thread(inputs).await?);
    *last.lock().await = Some(CachedSeasonView::new(view.clone()));
    Ok(view)
}

#[cfg(test)]
mod tests {
    use super::{cache_is_usable, CACHED_VIEW_MAX_AGE_SECS};

    #[test]
    fn a_remembered_view_is_reused_until_it_is_stale() {
        let built = 1_700_000_000;
        assert!(cache_is_usable("league-a", built, "league-a", built));
        assert!(cache_is_usable(
            "league-a",
            built,
            "league-a",
            built + CACHED_VIEW_MAX_AGE_SECS - 1
        ));
        // The bug: chat answered from whatever the season screen last built,
        // however many hours ago that was.
        assert!(!cache_is_usable(
            "league-a",
            built,
            "league-a",
            built + CACHED_VIEW_MAX_AGE_SECS
        ));
        assert!(!cache_is_usable("league-a", built, "league-b", built));
        // A clock that moved backwards is not a reason to throw it away.
        assert!(cache_is_usable("league-a", built, "league-a", built - 60));
    }
}
