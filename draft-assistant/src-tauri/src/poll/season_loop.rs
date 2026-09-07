//! One turn of the season poll loop.
//!
//! Split out of `poll.rs`, which is at the line cap, and kept away from the
//! command layer so a test can drive a whole tick against a loader that fails,
//! rolls the week over, or hands back a changed injury report on demand.

mod rollover;

pub use rollover::{refresh_or_roll, reload_for_week};

use super::{AnalysisCache, LiveEmitGate, PollHealth, PollHealthMemory};
use crate::engine::{now_secs, AppConfig, LoadedLeague};
use crate::season_engine::week_watch::{Watch, CHECK_EVERY_SECS, PLAYERS_EVERY_SECS};
use crate::season_engine::{LoadedSeason, SeasonLoader};
use crate::season_history::HistoryStore;
use crate::season_refresh::{wanted_ids, PlayerRefresh};
use crate::state::{build_season_cached_off_thread, season_inputs};
use std::collections::hash_map::DefaultHasher;
use std::hash::{Hash, Hasher};
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
    scoreboard: ScoreboardWatch,
    week: Watch,
    players: Watch,
    builds: u32,
    /// The [`LoadedSeason::epoch`] of the season the last tick looked at.
    epoch: Option<u64>,
}

/// The raw scoreboard-and-scoring signature the last tick saw.
///
/// Read off the fetched data rather than off a built view, which is the whole
/// point: it decides whether there is anything worth building at all.
#[derive(Debug, Default)]
struct ScoreboardWatch {
    last: Option<u64>,
}

impl ScoreboardWatch {
    /// True when the scoring or the scoreboard differs from the last tick.
    /// The first tick counts as a change, so the screen gets its initial view.
    fn moved(&mut self, signature: u64) -> bool {
        if self.last == Some(signature) {
            return false;
        }
        self.last = Some(signature);
        true
    }
}

/// A cheap stand-in for everything the live half of a season view is built
/// out of: every roster's total this week, and the state of every NFL game.
///
/// Deliberately coarser than [`crate::poll::LiveEmitGate`], which prices only
/// my matchup: this one is asked *before* anything is solved, so it cannot
/// know which two rosters matter without doing the work it is trying to
/// avoid. Being coarse only ever costs a build that would have been
/// suppressed a moment later; it can never miss a change.
fn live_signature(season: &LoadedSeason) -> u64 {
    let mut hasher = DefaultHasher::new();
    for matchup in season.matchups.iter() {
        matchup.roster_id.hash(&mut hasher);
        (matchup.scored() * 100.0)
            .round()
            .to_bits()
            .hash(&mut hasher);
        // Per-player scoring as well as the team total. The two live totals
        // on the header are summed out of these, and a total that has not
        // moved is not proof that nobody scored: a starter swapped in after
        // kickoff moves one of these and neither of the others.
        //
        // Sorted because Sleeper's per-player points arrive as a map, whose
        // iteration order differs between processes and would otherwise make
        // this signature random.
        let mut points: Vec<(&str, u64)> = matchup
            .players_points
            .iter()
            .flatten()
            .map(|(id, points)| (id.as_str(), (points * 100.0).round().to_bits()))
            .collect();
        points.sort_unstable();
        points.hash(&mut hasher);
    }
    // The scoreboard has no maps in it, so its JSON is field-ordered and
    // stable between ticks. Sixteen games is nothing to serialise once every
    // thirty seconds, and spelling every field out by hand would rot the
    // moment Sleeper adds one.
    serde_json::to_string(&season.scores)
        .unwrap_or_default()
        .hash(&mut hasher);
    hasher.finish()
}

impl SeasonPollMemory {
    /// `rebuild_every` is how many ticks the cached analysis is reused for.
    pub fn new(rebuild_every: u32) -> Self {
        Self {
            health: PollHealthMemory::default(),
            gate: LiveEmitGate::default(),
            analysis: AnalysisCache::new(rebuild_every),
            scoreboard: ScoreboardWatch::default(),
            week: Watch::every(CHECK_EVERY_SECS),
            players: Watch::every(PLAYERS_EVERY_SECS),
            builds: 0,
            epoch: None,
        }
    }

    /// Note which season this tick is looking at.
    ///
    /// A season with an epoch the poller has not seen was put there by a
    /// command: a forced reload from the screen, a league switch, a rollover.
    /// Everything remembered here is about the season that was thrown away,
    /// so it goes with it. It used to stay: the scoreboard signature had not
    /// moved, the analysis was still inside its window, and the standings and
    /// waiver targets of the old season were re-emitted for up to twenty
    /// ticks over the new one.
    fn adopt(&mut self, epoch: u64) {
        if self.epoch == Some(epoch) {
            return;
        }
        self.epoch = Some(epoch);
        self.analysis.invalidate();
        self.scoreboard = ScoreboardWatch::default();
    }

    /// How many season views this poller has actually built.
    ///
    /// The only way from outside to tell a tick that built a view and then
    /// suppressed the emit from one that never built anything at all, which
    /// is exactly what the quiet-tick rule is about.
    pub fn builds(&self) -> u32 {
        self.builds
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
    season_ref: &Mutex<Option<LoadedSeason>>,
    league_id: &str,
    season: u32,
) -> bool {
    let Some(refreshed) = engine.refresh_players(season).await else {
        return false;
    };
    let mut loaded = loaded_ref.lock().await;
    let Some(loaded) = loaded.as_mut() else {
        return false;
    };
    if loaded.league.league_id != league_id {
        return false;
    }
    // Locked in the usual order, loaded then season, and only to read which
    // players this league actually cares about.
    let mut season_guard = season_ref.lock().await;
    let roster_ids: Vec<&str> = season_guard
        .as_ref()
        .map(|s| {
            s.rosters
                .iter()
                .flat_map(|r| r.player_ids())
                .map(String::as_str)
                .collect()
        })
        .unwrap_or_default();
    let usable = {
        let wanted = wanted_ids(
            loaded.board.iter().map(|p| p.player_id.as_str()),
            roster_ids.iter().copied(),
        );
        refreshed.is_usable(&wanted)
    };
    if !usable {
        // Keeping the old dictionary is the right answer, but silently is
        // not: a half-parsed refresh means the names and injury tags on
        // screen are as old as the league load, and the user has no other
        // way to find that out.
        if let Some(season) = season_guard.as_mut() {
            if !season.warnings.iter().any(|w| w == INCOMPLETE_PLAYERS) {
                season.warnings.push(INCOMPLETE_PLAYERS.to_string());
            }
        }
        return false;
    }
    // A good refresh takes the complaint down again. It used to stay on the
    // badge for the life of the process, so one truncated download at noon
    // still read "player list incomplete" at midnight over names and tags
    // that had been refreshed a dozen times since.
    if let Some(season) = season_guard.as_mut() {
        season.warnings.retain(|w| w != INCOMPLETE_PLAYERS);
    }
    drop(season_guard);
    refreshed.apply(loaded);
    true
}

/// The warning a refused player refresh leaves on the health badge, until the
/// next refresh that is worth applying.
pub const INCOMPLETE_PLAYERS: &str = "the player list came back incomplete, names and injury tags are the ones loaded with the league";

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
        season.as_ref().map(|s| (s.season, s.week, s.epoch))
    };
    let Some(mut watching) = read_watching().await else {
        return SeasonTick::default();
    };
    memory.adopt(watching.2);

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
                match reload_for_week(engine, loaded_ref, season_ref, config_ref, &league_id).await
                {
                    Ok(true) => {
                        // The live fetch below has to ask for the new week,
                        // not the one this tick started on.
                        let Some(now_watching) = read_watching().await else {
                            return SeasonTick::default();
                        };
                        watching = now_watching;
                        memory.adopt(watching.2);
                        week_changed = true;
                    }
                    // The league changed under the load; nothing to say.
                    Ok(false) => return SeasonTick::default(),
                    // Said out loud, and then the week we do have carries on
                    // being refreshed: the analysis was dropped above, so the
                    // warning reaches the screen on this tick's rebuild.
                    Err(error) => {
                        rollover::note_failure(season_ref, &league_id, watching.1, week, &error)
                            .await;
                    }
                }
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
        if refresh_players(engine, loaded_ref, season_ref, &league_id, watching.0).await {
            memory.analysis.invalidate();
        }
    }
    // The three requests run with nothing locked. Each has an eight-second
    // timeout and retries, so holding `season` across them stalled every
    // command that needs it and queued the next tick behind this one.
    let fetch_started = std::time::Instant::now();
    let fetched = engine.fetch_live(&league_id, watching.0, watching.1).await;
    // The tick boundary at the verbose level: the league and week, what each
    // of the three sources said, and how long the round trip took.
    crate::applog::debug(format!(
        "season tick fetched league={league_id} week={} matchups={} scores={} rosters={} in {}ms",
        watching.1,
        fetched.matchups.as_ref().err().map_or("ok", String::as_str),
        fetched.scores.as_ref().err().map_or("ok", String::as_str),
        fetched.rosters.as_ref().err().map_or("ok", String::as_str),
        fetch_started.elapsed().as_millis()
    ));
    let mut errors = Vec::new();
    let signature = {
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
        live_signature(season)
    };
    memory.health.record(&errors);
    let health = Some(memory.health.report());
    if !errors.is_empty() {
        return SeasonTick { view: None, health };
    }

    // Nothing on the scoreboard moved and the held analysis is still good, so
    // there is nothing to build. This used to be decided the other way round:
    // the whole view was built — the two lineup solves, and on a rebuild tick
    // the thousand-odd solves and the playoff simulation behind it — and only
    // then compared against the last one and thrown away. All night Tuesday
    // through Saturday that is a minute of CPU an hour spent to emit nothing.
    let moved = memory.scoreboard.moved(signature);
    if !moved && !memory.analysis.is_stale() {
        // The rebuild clock still has to run, or a screen left open would
        // never expire the analysis it is holding.
        memory.analysis.count_tick();
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
    memory.builds += 1;
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
