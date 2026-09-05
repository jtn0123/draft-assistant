//! Fetches and caches everything the in-season screen needs.
//!
//! Split by volatility, because these have wildly different refresh needs:
//! rosters and the week's matchups move on a scoring cadence, the full-season
//! matchup sweep only changes when a week ends, and last season never changes
//! at all. Each is cached with a TTL that matches.

mod last_season;
mod rows;
pub mod week_watch;

use crate::cache::safe_key;
use crate::engine::{now_secs, Engine, REQUEST_CONCURRENCY};
use crate::season::LastSeasonRow;
use crate::season_api::{Matchup, NflState, Roster, ScoreGame, SeasonEndpoints, Transaction};
use crate::season_history::History;
use crate::season_sources::{LiveFetch, SourceHealth};
use crate::sleeper::League;
use crate::sleeper_error::to_message;
use futures_util::StreamExt;
use rows::{merge_transactions, pairs_from};

use std::collections::HashMap;
use std::sync::Arc;

/// Live scoring windows move fast; everything else can lag a little.
const LIVE_TTL_SECS: u64 = 30;
const WEEK_SWEEP_TTL_SECS: u64 = 6 * 3600;
/// Where the NFL is, cached on disk. Not a freshness window — the copy is only
/// ever read when the live request fails — so the whole season is fair game:
/// last week's answer beats no screen at all.
const NFL_STATE_CACHE: &str = "nfl_state.json";
pub(crate) const LAST_SEASON_TTL_SECS: u64 = 30 * 24 * 3600;

/// week -> the (home_roster_id, away_roster_id) pairings played that week.
pub type WeekPairings = Vec<(u32, Vec<(u32, u32)>)>;

/// Everything in-season, already fetched.
///
/// The big collections are behind `Arc` because this struct is cloned on every
/// poll tick: the tick copies its inputs out from under the mutexes so the
/// build can run with nothing locked, and a deep copy of fifteen weeks of
/// matchups, a season of transactions and the whole Trends history every
/// thirty seconds is megabytes of pure waste. Nothing downstream of the copy
/// writes to any of them; the live refresh replaces them wholesale instead.
#[derive(Clone, Default)]
pub struct LoadedSeason {
    pub week: u32,
    pub season: u32,
    pub rosters: Arc<Vec<Roster>>,
    /// This week's matchup rows, one per roster.
    pub matchups: Arc<Vec<Matchup>>,
    /// week -> (home_roster_id, away_roster_id) pairings, all regular-season
    /// weeks. Future weeks feed the playoff simulation.
    pub schedule: Arc<WeekPairings>,
    /// player_id -> fantasy points scored so far this season.
    pub season_points: Arc<HashMap<String, f64>>,
    pub transactions: Arc<Vec<Transaction>>,
    pub scores: Arc<Vec<ScoreGame>>,
    pub last_season: Arc<Vec<LastSeasonRow>>,
    /// Strength-over-time snapshots for the Trends tab; filled in by the
    /// command layer after a load, empty in headless contexts.
    pub history: Arc<History>,
    pub fetched_at: u64,
    pub warnings: Vec<String>,
    /// When each live source last answered, and why it last did not.
    pub sources: SourceHealth,
}

/// The full-season matchup sweep, assembled from the per-week caches.
struct WeekSweep {
    schedule: WeekPairings,
    season_points: HashMap<String, f64>,
}

/// Loading and refreshing a season, as distinct from loading a draft.
///
/// Stated as a trait so the season sweep is a declared extension of `Engine`
/// rather than another anonymous `impl` block bolted onto it.
pub trait SeasonLoader {
    /// Load everything the season screen needs, from cache where possible.
    #[allow(async_fn_in_trait)]
    async fn load_season(
        &self,
        league: &League,
        my_user_id: Option<&str>,
        force: bool,
    ) -> Result<LoadedSeason, String>;

    /// Which NFL week it is now.
    ///
    /// Asked again while the app is running, not only at load: a season opened
    /// on Monday and left open used to keep scoring Monday's week through
    /// Tuesday's rollover and all of the next Sunday.
    #[allow(async_fn_in_trait)]
    async fn current_week(&self) -> Result<u32, String>;

    /// Pull the fast-moving slice — this week's scoring and the NFL scoreboard
    /// — without touching any shared state. Callers that hold the season
    /// behind a lock run this first, with nothing locked, and fold the result
    /// in afterwards.
    #[allow(async_fn_in_trait)]
    async fn fetch_live(&self, league_id: &str, season: u32, week: u32) -> LiveFetch;

    /// Fetch and fold in one step, for callers that already own the season
    /// outright. `Err` when every request failed.
    #[allow(async_fn_in_trait)]
    async fn refresh_live(&self, season: &mut LoadedSeason, league_id: &str) -> Result<(), String> {
        let fetched = self.fetch_live(league_id, season.season, season.week).await;
        fetched.apply(season, now_secs())
    }
}

impl Engine {
    fn season_cache_name(league_id: &str, suffix: &str) -> String {
        format!("season_{}_{suffix}.json", safe_key(league_id))
    }

    /// One week's matchup rows, cached on their own.
    ///
    /// A week that is already over can never change again, so it is kept
    /// forever; the current week and the ones still to come keep the old
    /// six-hour TTL. That is what makes a weekly rollover cost one request
    /// instead of fifteen — the sweep used to be a single blob stamped with
    /// the week it was taken in, so the week ticking over threw all of it
    /// away.
    async fn week_matchups(
        &self,
        league_id: &str,
        week: u32,
        current_week: u32,
        force: bool,
    ) -> Result<Vec<Matchup>, String> {
        let name = Self::season_cache_name(league_id, &format!("week{week}"));
        let settled = week < current_week;
        let ttl = if settled {
            u64::MAX
        } else {
            WEEK_SWEEP_TTL_SECS
        };
        if !force {
            if let Some((_, matchups)) = self.read_cache::<Vec<Matchup>>(&name, ttl) {
                return Ok(matchups);
            }
        }
        let matchups = self
            .client
            .matchups(league_id, week)
            .await
            .map_err(to_message)?;
        self.write_cache(&name, &matchups);
        Ok(matchups)
    }

    /// Sweep every regular-season week: pairings for the simulation and
    /// season-to-date points per player. Weeks already on disk cost nothing.
    async fn week_sweep(
        &self,
        league_id: &str,
        week: u32,
        last_regular_week: u32,
        force: bool,
        warnings: &mut Vec<String>,
    ) -> WeekSweep {
        let mut schedule = Vec::new();
        let mut season_points: HashMap<String, f64> = HashMap::new();
        let mut failed = Vec::new();

        // Fifteen-odd weeks, six requests at a time rather than one after
        // another; the results come back out of order, so sort before use.
        let mut fetched: Vec<(u32, Result<Vec<Matchup>, String>)> =
            futures_util::stream::iter(1..=last_regular_week.max(week))
                .map(|w| async move { (w, self.week_matchups(league_id, w, week, force).await) })
                .buffer_unordered(REQUEST_CONCURRENCY)
                .collect()
                .await;
        fetched.sort_by_key(|(w, _)| *w);

        for (w, result) in fetched {
            match result {
                Ok(matchups) => {
                    schedule.push((w, pairs_from(&matchups)));
                    // Only weeks already played contribute points.
                    if w <= week {
                        for m in &matchups {
                            for (player_id, points) in m.players_points.iter().flatten() {
                                *season_points.entry(player_id.clone()).or_insert(0.0) += points;
                            }
                        }
                    }
                }
                Err(_) => failed.push(w),
            }
        }
        if !failed.is_empty() {
            warnings.push(format!(
                "matchups unavailable for week{} {} \u{2014} playoff odds and season totals are approximate",
                if failed.len() == 1 { "" } else { "s" },
                failed
                    .iter()
                    .map(u32::to_string)
                    .collect::<Vec<_>>()
                    .join(", ")
            ));
        }
        WeekSweep {
            schedule,
            season_points,
        }
    }

    /// Load the whole in-season picture for a league.
    ///
    /// Individual panels degrade rather than failing the load: a transactions
    /// outage costs you the activity feed, not the screen.
    /// How stale the live slice is, in seconds.
    pub fn live_age(season: &LoadedSeason) -> u64 {
        now_secs().saturating_sub(season.fetched_at)
    }

    /// True when the live slice is old enough to be worth re-fetching.
    pub fn live_is_stale(season: &LoadedSeason) -> bool {
        Self::live_age(season) >= LIVE_TTL_SECS
    }

    /// Every roster in a league, from the network when it answers and from the
    /// last good copy on disk when it does not.
    ///
    /// This was the one request in the whole load with no fallback, so a
    /// season that could otherwise have opened from cache — the week, the
    /// matchups, the sweep, last season, all on disk — failed outright on a
    /// train with no signal. Rosters barely move between waiver runs, so
    /// yesterday's copy with a staleness warning beats no screen at all.
    async fn rosters_or_cached(
        &self,
        league_id: &str,
    ) -> Result<(Vec<Roster>, Option<String>), String> {
        let name = Self::season_cache_name(league_id, "rosters");
        match self.client.rosters(league_id).await {
            Ok(rosters) => {
                self.write_cache_off_thread(&name, &rosters).await;
                Ok((rosters, None))
            }
            Err(error) => {
                let error = to_message(error);
                let Some((at, rosters)) =
                    self.read_cache_any_off_thread::<Vec<Roster>>(&name).await
                else {
                    return Err(error);
                };
                let age_hours = now_secs().saturating_sub(at) / 3600;
                Ok((
                    rosters,
                    Some(format!(
                        "rosters could not be refreshed ({error}) \u{2014} showing the ones last seen {age_hours}h ago"
                    )),
                ))
            }
        }
    }

    /// Where the NFL is, from the network when it answers and from the last
    /// good copy on disk when it does not.
    ///
    /// This one request used to fail the whole season load: offline, or with
    /// Sleeper down, the screen went from "stale but readable" to nothing at
    /// all, because the week is the first thing every other request needs.
    /// The fallback comes with the usual stale warning rather than quietly
    /// pretending the week is current.
    pub(crate) async fn nfl_state_or_cached(&self) -> Result<(NflState, Option<String>), String> {
        match self.client.nfl_state().await {
            Ok(state) => {
                self.write_cache_off_thread(NFL_STATE_CACHE, &state).await;
                Ok((state, None))
            }
            Err(error) => {
                let error = to_message(error);
                let Some((at, state)) = self
                    .read_cache_any_off_thread::<NflState>(NFL_STATE_CACHE)
                    .await
                else {
                    return Err(error);
                };
                let age_hours = now_secs().saturating_sub(at) / 3600;
                Ok((
                    state,
                    Some(format!(
                        "which NFL week it is could not be checked ({error}) \u{2014} showing the week last seen {age_hours}h ago"
                    )),
                ))
            }
        }
    }
}

impl SeasonLoader for Engine {
    async fn current_week(&self) -> Result<u32, String> {
        self.nfl_state_or_cached()
            .await
            .map(|(state, _)| state.current_week())
    }

    async fn load_season(
        &self,
        league: &League,
        my_user_id: Option<&str>,
        force: bool,
    ) -> Result<LoadedSeason, String> {
        let (state, stale_week) = self.nfl_state_or_cached().await?;
        let week = state.current_week();
        let season: u32 = league
            .season
            .parse()
            .map_err(|_| format!("league season '{}' is not a year", league.season))?;
        let league_id = league.league_id.as_str();
        let mut warnings = Vec::new();
        warnings.extend(stale_week);

        let (rosters, matchups, scores) = tokio::join!(
            self.rosters_or_cached(league_id),
            self.client.matchups(league_id, week),
            self.client.nfl_scores(season, week)
        );
        let (rosters, stale_rosters) = rosters?;
        // The same per-source bookkeeping the live poll keeps, so the health
        // badge starts out honest rather than waiting for the first refresh.
        let loaded_at = now_secs();
        let mut sources = SourceHealth::default();
        match &stale_rosters {
            // Rosters served from disk are not a live source, and the badge
            // has to say so or it would vouch for a lineup that may be a day
            // out of date.
            Some(note) => sources.rosters.failed(note.clone()),
            None => sources.rosters.succeeded(loaded_at),
        }
        warnings.extend(stale_rosters);
        let matchups = match matchups {
            Ok(matchups) => {
                sources.matchups.succeeded(loaded_at);
                matchups
            }
            Err(error) => {
                warnings.push(format!("this week's matchups unavailable: {error}"));
                sources.matchups.failed(error.to_string());
                Vec::new()
            }
        };
        let scores = match scores {
            Ok(scores) => {
                sources.scores.succeeded(loaded_at);
                scores
            }
            Err(error) => {
                warnings.push(format!("live NFL scores unavailable: {error}"));
                sources.scores.failed(error.to_string());
                Vec::new()
            }
        };

        // The activity feed spans this week and last, which is what "recent"
        // means to someone checking waivers. The two weeks go out together
        // rather than one after the other, the way every sibling path does.
        // In week 1 "last week" is week 1 too; fetching it twice would list
        // every preseason move twice, so week 1 asks once.
        let previous = week.saturating_sub(1).max(1);
        let batches: Vec<(u32, Result<Vec<Transaction>, String>)> = if previous == week {
            vec![(
                week,
                self.client
                    .transactions(league_id, week)
                    .await
                    .map_err(to_message),
            )]
        } else {
            let (earlier, current) = tokio::join!(
                self.client.transactions(league_id, previous),
                self.client.transactions(league_id, week)
            );
            vec![
                (previous, earlier.map_err(to_message)),
                (week, current.map_err(to_message)),
            ]
        };
        let transactions = merge_transactions(batches, &mut warnings);

        let sweep = self
            .week_sweep(
                league_id,
                week,
                league.last_regular_week(),
                force,
                &mut warnings,
            )
            .await;
        let last_season = self.last_season(league, my_user_id, force).await;

        Ok(LoadedSeason {
            week,
            season,
            rosters: Arc::new(rosters),
            matchups: Arc::new(matchups),
            schedule: Arc::new(sweep.schedule),
            season_points: Arc::new(sweep.season_points),
            transactions: Arc::new(transactions),
            scores: Arc::new(scores),
            last_season: Arc::new(last_season),
            history: Arc::new(History::default()),
            fetched_at: loaded_at,
            warnings,
            sources,
        })
    }

    /// Refresh only the fast-moving parts: this week's scoring and the NFL
    /// scoreboard. Used by the in-season poller.
    ///
    /// Which endpoint gave what, and what that means for the staleness clock,
    /// is decided in `season_sources` where it can be tested without a
    /// network.
    async fn fetch_live(&self, league_id: &str, season: u32, week: u32) -> LiveFetch {
        let (matchups, scores, rosters) = tokio::join!(
            self.client.matchups(league_id, week),
            self.client.nfl_scores(season, week),
            self.client.rosters(league_id)
        );
        LiveFetch {
            matchups: matchups.map_err(to_message),
            scores: scores.map_err(to_message),
            rosters: rosters.map_err(to_message),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::projections::test_support::offline_engine;
    use crate::season_engine::rows::matchup;

    /// The rollover fix: a week that is over can never change, so its rows
    /// stand however old the copy is. Only the week being played expires.
    #[tokio::test]
    async fn a_finished_week_is_never_refetched_but_the_current_one_expires() {
        let engine = offline_engine("week-cache");
        let stale = now_secs() - WEEK_SWEEP_TTL_SECS - 1;
        for week in [3u32, 5] {
            let name = Engine::season_cache_name("league-1", &format!("week{week}"));
            crate::cache::write_atomic(
                engine.data_dir.join(format!("{name}.tmp")),
                engine.data_dir.join(&name),
                stale,
                &vec![matchup(1, Some(1))],
            )
            .unwrap();
        }

        let settled = engine
            .week_matchups("league-1", 3, 5, false)
            .await
            .expect("a finished week is served from disk at any age");
        assert_eq!(settled.len(), 1);

        // Week 5 is being played, so a six-hour-old copy is refetched — and
        // offline that fails rather than passing stale scoring off as live.
        assert!(engine.week_matchups("league-1", 5, 5, false).await.is_err());
        std::fs::remove_dir_all(engine.data_dir).unwrap();
    }
}
