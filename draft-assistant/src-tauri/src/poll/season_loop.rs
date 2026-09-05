//! One turn of the season poll loop.
//!
//! Split out of `poll.rs`, which is at the line cap, and kept away from the
//! command layer so a test can drive a whole tick against a loader that fails,
//! rolls the week over, or hands back a changed injury report on demand.

use super::{AnalysisCache, LiveEmitGate, PollHealth, PollHealthMemory};
use crate::engine::{now_secs, AppConfig, LoadedLeague};
use crate::season_engine::week_watch::{Watch, CHECK_EVERY_SECS, PLAYERS_EVERY_SECS};
use crate::season_engine::{LoadedSeason, SeasonLoader};
use crate::season_history::HistoryStore;
use crate::season_refresh::PlayerRefresh;
use crate::state::{build_season_cached_off_thread, season_inputs};
use tokio::sync::Mutex;

/// Everything the season poller needs of the engine: the season feeds, the
/// Trends file, and the slow-moving player data.
///
/// One alias rather than three bounds repeated on every function here.
pub trait SeasonEngine: SeasonLoader + HistoryStore + PlayerRefresh {}
impl<T: SeasonLoader + HistoryStore + PlayerRefresh> SeasonEngine for T {}

/// What the season poller remembers between ticks: whether it is getting
/// through, what it last emitted, the analysis it is reusing, and when it last
/// asked the two slow questions.
#[derive(Debug)]
pub struct SeasonPollMemory {
    health: PollHealthMemory,
    gate: LiveEmitGate,
    analysis: AnalysisCache,
    week: Watch,
    players: Watch,
}

impl SeasonPollMemory {
    /// `rebuild_every` is how many ticks the cached analysis is reused for.
    pub fn new(rebuild_every: u32) -> Self {
        Self {
            health: PollHealthMemory::default(),
            gate: LiveEmitGate::default(),
            analysis: AnalysisCache::new(rebuild_every),
            week: Watch::every(CHECK_EVERY_SECS),
            players: Watch::every(PLAYERS_EVERY_SECS),
        }
    }
}

/// What one season tick decided the app should be told.
#[derive(Debug, Default)]
pub struct SeasonTick {
    /// The view worth emitting, or `None` when the scores have not moved.
    pub view: Option<crate::season::SeasonView>,
    /// How the refresh went, or `None` when there was nothing to refresh — no
    /// league open yet, or the season not loaded. Neither is the feed failing,
    /// so neither should be reported as one.
    pub health: Option<PollHealth>,
}

/// Reload the whole season for a week that has just turned over, replacing
/// what the poller was watching. `false` when the reload failed or the league
/// changed underneath it — either way there is nothing to emit this tick.
///
/// The load runs with nothing locked, exactly like the live fetch: it is
/// fifteen matchup requests and can take seconds, and holding the season
/// across it would stall every command in the app.
async fn reload_for_week<E: SeasonEngine>(
    engine: &E,
    loaded_ref: &Mutex<Option<LoadedLeague>>,
    season_ref: &Mutex<Option<LoadedSeason>>,
    config_ref: &Mutex<AppConfig>,
    league_id: &str,
) -> bool {
    let league = {
        let loaded = loaded_ref.lock().await;
        match loaded.as_ref() {
            Some(l) if l.league.league_id == league_id => l.league.clone(),
            _ => return false,
        }
    };
    let my_user_id = config_ref.lock().await.my_user_id.clone();
    let Ok(mut fresh) = engine
        .load_season(&league, my_user_id.as_deref(), false)
        .await
    else {
        return false;
    };
    // Checked again on the way back in: the load ran unlocked, and writing
    // this would otherwise file one league's rosters under another's.
    //
    // The league is copied out rather than held, because recording the Trends
    // snapshot below reads that file, diffs it and writes it back.
    let mine = {
        let loaded = loaded_ref.lock().await;
        match loaded.as_ref() {
            Some(l) if l.league.league_id == league_id => l.clone(),
            _ => return false,
        }
    };
    // `Engine::load_season` hands back an empty history, because the file it
    // lives in is the command layer's business. The user-driven load fills it
    // in; the automatic rollover did not, so every Tuesday morning the Trends
    // tab silently emptied itself and the week just finished was never
    // recorded at all.
    fresh.history = engine.record_history(&mine, &fresh).await;
    *season_ref.lock().await = Some(fresh);
    true
}

/// Re-read the player dictionary and the weekly projections, and swap them
/// into the loaded league.
///
/// Runs with nothing locked — the dictionary alone is ~14.6 MB — and takes
/// the `loaded` mutex only to apply the result, which keeps this on the
/// loaded -> season -> config order the rest of the app uses.
/// `true` when something was actually swapped in.
async fn refresh_players<E: SeasonEngine>(
    engine: &E,
    loaded_ref: &Mutex<Option<LoadedLeague>>,
    league_id: &str,
    season: u32,
) -> bool {
    let Some(refreshed) = engine.refresh_players(season).await else {
        return false;
    };
    if !refreshed.is_usable() {
        return false;
    }
    let mut loaded = loaded_ref.lock().await;
    let Some(loaded) = loaded.as_mut() else {
        return false;
    };
    if loaded.league.league_id != league_id {
        return false;
    }
    refreshed.apply(loaded);
    true
}

/// One turn of the season poll loop: refresh the live slice, note whether that
/// worked, and rebuild the view if the scores moved.
///
/// The loop around this lives in the command layer because it needs Tauri's
/// event emitter; everything it decides lives here, where a test can drive it
/// with a loader that fails on demand.
pub async fn season_tick<E: SeasonEngine>(
    engine: &E,
    loaded_ref: &Mutex<Option<LoadedLeague>>,
    season_ref: &Mutex<Option<LoadedSeason>>,
    config_ref: &Mutex<AppConfig>,
    memory: &mut SeasonPollMemory,
) -> SeasonTick {
    let league_id = {
        let loaded = loaded_ref.lock().await;
        loaded.as_ref().map(|l| l.league.league_id.clone())
    };
    let Some(league_id) = league_id else {
        return SeasonTick::default();
    };

    let read_watching = || async {
        let season = season_ref.lock().await;
        season.as_ref().map(|s| (s.season, s.week))
    };
    let Some(mut watching) = read_watching().await else {
        return SeasonTick::default();
    };

    // Has the NFL moved on? Checked on a ten-minute clock of its own, because
    // the answer changes once a week and a poll tick is thirty seconds. A new
    // week is not a live refresh — every roster, matchup and projection is a
    // different week's — so it goes down the full load path and the analysis
    // held from the old week is dropped with it.
    let mut week_changed = false;
    if memory.week.due(now_secs()) {
        memory.week.checked(now_secs());
        if let Ok(week) = engine.current_week().await {
            if week != watching.1 {
                memory.analysis.invalidate();
                if !reload_for_week(engine, loaded_ref, season_ref, config_ref, &league_id).await {
                    return SeasonTick::default();
                }
                // The live fetch below has to ask for the new week, not the
                // one this tick started on.
                let Some(now_watching) = read_watching().await else {
                    return SeasonTick::default();
                };
                watching = now_watching;
                week_changed = true;
            }
        }
    }
    // Injury statuses and weekly projections were fetched once, by the load
    // that opened the league, and never again: an app left open all Sunday
    // scored a starter who had been ruled out on Saturday night. Re-read on a
    // half-hour clock, and always the moment the week turns over — the new
    // week's projections are the whole point of the new week.
    if week_changed || memory.players.due(now_secs()) {
        memory.players.checked(now_secs());
        // Waiver targets, trade ideas and playoff odds are all built out of
        // those projections and those injury tags, so the copy held from
        // before the refresh is stale by definition. Dropping it is also what
        // gets the change on screen: the emit gate watches the score and the
        // scoreboard, and a Saturday downgrade moves neither.
        if refresh_players(engine, loaded_ref, &league_id, watching.0).await {
            memory.analysis.invalidate();
        }
    }
    // The three requests run with nothing locked. Each has an eight-second
    // timeout and retries, so holding `season` across them stalled every
    // command that needs it and queued the next tick behind this one.
    let fetched = engine.fetch_live(&league_id, watching.0, watching.1).await;
    let mut errors = Vec::new();
    {
        // Locks in the usual order, loaded then season. The league is checked
        // again because the requests ran unlocked: after a switch this is the
        // old league's scoring, and folding it in would show one league's
        // live points on another league's screen. Nothing was applied, so
        // nothing is reported either.
        let loaded = loaded_ref.lock().await;
        if loaded.as_ref().map(|l| l.league.league_id.as_str()) != Some(league_id.as_str()) {
            return SeasonTick::default();
        }
        let mut season = season_ref.lock().await;
        let Some(season) = season.as_mut() else {
            return SeasonTick::default();
        };
        if let Err(error) = fetched.apply(season, now_secs()) {
            errors.push(error);
        }
    }
    memory.health.record(&errors);
    let health = Some(memory.health.report());
    if !errors.is_empty() {
        return SeasonTick { view: None, health };
    }

    // The inputs are copied under the three mutexes — taken loaded -> season
    // -> config, the same order as everywhere else — and the build itself runs
    // on the blocking pool with none of them held. It is seconds of lineup
    // solving and playoff simulation, and doing it on a runtime thread stopped
    // the draft poller and every command for the length of every tick.
    let Ok(inputs) = season_inputs(loaded_ref, season_ref, config_ref).await else {
        return SeasonTick { view: None, health };
    };
    let cached = memory.analysis.get().cloned();
    let Ok(view) = build_season_cached_off_thread(inputs, cached).await else {
        return SeasonTick { view: None, health };
    };
    memory.analysis.observe(&view);
    let moved = memory.gate.should_emit(
        view.live.totals.my_live_points,
        view.live.totals.opp_live_points,
        &view.live.games,
        memory.analysis.generation(),
    );
    SeasonTick {
        view: moved.then_some(view),
        health,
    }
}
