//! The harness both season-poll test binaries drive: a loader that can be made
//! to fail, roll the week over, or hand back a changed player dictionary.
//!
//! Its own module because a single file holding it plus every test would be
//! over the repository's line cap.

use draft_assistant_lib::engine::{Engine, LoadedLeague};
use draft_assistant_lib::poll::{season_tick, SeasonPollMemory, SeasonTick};
use draft_assistant_lib::season_api::{Matchup, Roster};
use draft_assistant_lib::season_engine::{LoadedSeason, SeasonLoader};
use draft_assistant_lib::season_history::{History, HistoryStore};
use draft_assistant_lib::season_refresh::{refresh_from, PlayerRefresh, PlayerRefreshData};
use draft_assistant_lib::season_sources::LiveFetch;
use draft_assistant_lib::sleeper::{League, PlayerMeta};
use std::cell::Cell;
use std::collections::HashMap;
use tokio::sync::Mutex;

/// A scratch data directory of this test binary's own.
pub fn scratch(label: &str) -> std::path::PathBuf {
    std::env::temp_dir().join(format!(
        "draft-assistant-season-poll-{label}-{}-{}",
        std::process::id(),
        std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .expect("a clock after 1970")
            .as_nanos()
    ))
}

/// A season loader whose live refresh fails whenever `failing` is set, and
/// which otherwise hands back the fixture's own rows unchanged.
pub struct Flaky {
    pub failing: Cell<bool>,
    pub matchups: Vec<Matchup>,
    pub rosters: Vec<Roster>,
    /// What `current_week` answers, and the season a rollover reload hands
    /// back. `None` on the season means the reload itself fails.
    pub week: Cell<u32>,
    pub reloads: Cell<u32>,
    pub reloaded: Option<LoadedSeason>,
    /// Where the Trends file lives. A real `Engine` keeps it, so the rollover
    /// path is exercised against the same reader and writer the user-driven
    /// load goes through rather than a stub of one.
    pub history: Engine,
    /// The dictionary the next player refresh hands back, or `None` for a
    /// refresh that finds nothing.
    pub players: Option<HashMap<String, PlayerMeta>>,
    pub refreshes: Cell<u32>,
}

impl SeasonLoader for Flaky {
    async fn load_season(
        &self,
        _league: &League,
        _my_user_id: Option<&str>,
        _force: bool,
    ) -> Result<LoadedSeason, String> {
        self.reloads.set(self.reloads.get() + 1);
        self.reloaded
            .clone()
            .ok_or_else(|| "the whole season load failed".to_string())
    }

    async fn current_week(&self) -> Result<u32, String> {
        if self.failing.get() {
            return Err("request failed".to_string());
        }
        Ok(self.week.get())
    }

    async fn fetch_live(&self, _league_id: &str, _season: u32, _week: u32) -> LiveFetch {
        if self.failing.get() {
            return LiveFetch {
                matchups: Err("request failed".into()),
                scores: Err("request failed".into()),
                rosters: Err("request failed".into()),
            };
        }
        LiveFetch {
            matchups: Ok(self.matchups.clone()),
            scores: Ok(Vec::new()),
            rosters: Ok(self.rosters.clone()),
        }
    }
}

impl HistoryStore for Flaky {
    async fn record_history(&self, loaded: &LoadedLeague, season: &LoadedSeason) -> History {
        self.history.record_history(loaded, season).await
    }
}

impl PlayerRefresh for Flaky {
    async fn refresh_players(&self, _season: u32) -> Option<PlayerRefreshData> {
        self.refreshes.set(self.refreshes.get() + 1);
        let players = self.players.clone()?;
        Some(refresh_from(players, 1, Vec::new(), 1))
    }
}

/// The three pieces of app state a tick reads, plus the loader driving it.
pub struct Harness {
    pub engine: Flaky,
    pub loaded: Mutex<Option<LoadedLeague>>,
    pub season: Mutex<Option<LoadedSeason>>,
    pub config: Mutex<draft_assistant_lib::engine::AppConfig>,
    pub memory: SeasonPollMemory,
}

impl Harness {
    pub fn named(label: &str) -> Self {
        let (loaded, season, config) = crate::common::fixture();
        Self {
            engine: Flaky {
                failing: Cell::new(false),
                matchups: season.matchups.as_ref().clone(),
                rosters: season.rosters.as_ref().clone(),
                week: Cell::new(season.week),
                reloads: Cell::new(0),
                reloaded: None,
                history: Engine::new(scratch(label)),
                players: None,
                refreshes: Cell::new(0),
            },
            loaded: Mutex::new(Some(loaded)),
            season: Mutex::new(Some(season)),
            config: Mutex::new(config),
            memory: SeasonPollMemory::new(20),
        }
    }

    pub async fn tick(&mut self) -> SeasonTick {
        season_tick(
            &self.engine,
            &self.loaded,
            &self.season,
            &self.config,
            &mut self.memory,
        )
        .await
    }
}

impl Drop for Harness {
    /// Each harness owns a scratch data directory for its Trends file; nothing
    /// it writes outlives the test that made it.
    fn drop(&mut self) {
        std::fs::remove_dir_all(&self.engine.history.data_dir).ok();
    }
}
