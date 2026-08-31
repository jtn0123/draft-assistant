//! Fetches and caches everything the in-season screen needs.
//!
//! Split by volatility, because these have wildly different refresh needs:
//! rosters and the week's matchups move on a scoring cadence, the full-season
//! matchup sweep only changes when a week ends, and last season never changes
//! at all. Each is cached with a TTL that matches.

use crate::cache::safe_key;
use crate::engine::{now_secs, Engine, REQUEST_CONCURRENCY};
use crate::season::LastSeasonRow;
use crate::season_api::{Matchup, Roster, ScoreGame, Transaction};
use crate::season_history::History;
use crate::season_sources::{apply_refresh, SourceHealth};
use crate::sleeper::League;
use futures_util::StreamExt;
use serde::{Deserialize, Serialize};
use std::collections::HashMap;

/// Live scoring windows move fast; everything else can lag a little.
const LIVE_TTL_SECS: u64 = 30;
const WEEK_SWEEP_TTL_SECS: u64 = 6 * 3600;
const LAST_SEASON_TTL_SECS: u64 = 30 * 24 * 3600;

/// Everything in-season, already fetched.
#[derive(Default)]
pub struct LoadedSeason {
    pub week: u32,
    pub season: u32,
    pub rosters: Vec<Roster>,
    /// This week's matchup rows, one per roster.
    pub matchups: Vec<Matchup>,
    /// week -> (home_roster_id, away_roster_id) pairings, all regular-season
    /// weeks. Future weeks feed the playoff simulation.
    pub schedule: Vec<(u32, Vec<(u32, u32)>)>,
    /// player_id -> fantasy points scored so far this season.
    pub season_points: HashMap<String, f64>,
    pub transactions: Vec<Transaction>,
    pub scores: Vec<ScoreGame>,
    pub last_season: Vec<LastSeasonRow>,
    /// Strength-over-time snapshots for the Trends tab; filled in by the
    /// command layer after a load, empty in headless contexts.
    pub history: History,
    pub fetched_at: u64,
    pub warnings: Vec<String>,
    /// When each live source last answered, and why it last did not.
    pub sources: SourceHealth,
}

/// The full-season matchup sweep, cached as one blob.
#[derive(Serialize, Deserialize)]
struct WeekSweep {
    week: u32,
    schedule: Vec<(u32, Vec<(u32, u32)>)>,
    season_points: HashMap<String, f64>,
}

/// Pair up the rosters sharing a matchup_id. Sleeper gives two rows per game
/// with no home/away distinction, so the lower roster id is treated as home —
/// arbitrary but stable, and the simulation is symmetric anyway.
fn pairs_from(matchups: &[Matchup]) -> Vec<(u32, u32)> {
    let mut by_id: HashMap<u32, Vec<u32>> = HashMap::new();
    for m in matchups {
        if let Some(id) = m.matchup_id {
            by_id.entry(id).or_default().push(m.roster_id);
        }
    }
    let mut pairs: Vec<(u32, u32)> = by_id
        .into_values()
        .filter_map(|mut rosters| {
            rosters.sort_unstable();
            match rosters.as_slice() {
                [home, away] => Some((*home, *away)),
                _ => None,
            }
        })
        .collect();
    pairs.sort_unstable();
    pairs
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

    /// Refresh only the fast-moving slice: this week's scoring and the NFL
    /// scoreboard. `Err` when every request failed.
    #[allow(async_fn_in_trait)]
    async fn refresh_live(&self, season: &mut LoadedSeason, league_id: &str) -> Result<(), String>;
}

impl Engine {
    fn season_cache_name(league_id: &str, suffix: &str) -> String {
        format!("season_{}_{suffix}.json", safe_key(league_id))
    }

    /// Sweep every regular-season week once: pairings for the simulation and
    /// season-to-date points per player. Cached — this is 15-ish requests.
    async fn week_sweep(
        &self,
        league_id: &str,
        week: u32,
        last_regular_week: u32,
        force: bool,
        warnings: &mut Vec<String>,
    ) -> WeekSweep {
        let name = Self::season_cache_name(league_id, "weeks");
        if !force {
            if let Some((_, sweep)) = self.read_cache::<WeekSweep>(&name, WEEK_SWEEP_TTL_SECS) {
                if sweep.week == week {
                    return sweep;
                }
            }
        }
        let mut schedule = Vec::new();
        let mut season_points: HashMap<String, f64> = HashMap::new();
        let mut failed = Vec::new();

        // Fifteen-odd weeks, six requests at a time rather than one after
        // another; the results come back out of order, so sort before use.
        let mut fetched: Vec<(u32, Result<Vec<Matchup>, String>)> =
            futures_util::stream::iter(1..=last_regular_week.max(week))
                .map(|w| async move { (w, self.client.matchups(league_id, w).await) })
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
        let sweep = WeekSweep {
            week,
            schedule,
            season_points,
        };
        self.write_cache(&name, &sweep);
        sweep
    }

    /// Last season's final table, from the previous league in the chain.
    async fn last_season(
        &self,
        league: &League,
        my_user_id: Option<&str>,
        force: bool,
    ) -> Vec<LastSeasonRow> {
        let Some(previous_id) = league.previous_league_id.as_deref() else {
            return Vec::new();
        };
        if previous_id.is_empty() || previous_id == "0" {
            return Vec::new();
        }
        let name = Self::season_cache_name(previous_id, "final");
        if !force {
            if let Some((_, rows)) =
                self.read_cache::<Vec<LastSeasonRow>>(&name, LAST_SEASON_TTL_SECS)
            {
                return rows;
            }
        }
        let (rosters, users, bracket) = tokio::join!(
            self.client.rosters(previous_id),
            self.client.league_users(previous_id),
            self.client.winners_bracket(previous_id)
        );
        let Ok(rosters) = rosters else {
            return Vec::new();
        };
        let names: HashMap<String, String> = users
            .unwrap_or_default()
            .into_iter()
            .filter_map(|u| u.label().map(|n| (u.user_id.clone(), n)))
            .collect();
        // The game that decides first place names the champion.
        let champion =
            bracket.unwrap_or_default().iter().find_map(
                |m| {
                    if m.p == Some(1) {
                        m.w
                    } else {
                        None
                    }
                },
            );
        let most_points = rosters
            .iter()
            .max_by(|a, b| a.settings.points_for().total_cmp(&b.settings.points_for()))
            .map(|r| r.roster_id);

        let mut ordered: Vec<&Roster> = rosters.iter().collect();
        // Champion first — they finished first overall whatever the regular
        // season said — then everyone else by record and points.
        ordered.sort_by(|a, b| {
            let champ = |r: &Roster| champion == Some(r.roster_id);
            champ(b)
                .cmp(&champ(a))
                .then_with(|| b.settings.wins.cmp(&a.settings.wins))
                .then_with(|| b.settings.points_for().total_cmp(&a.settings.points_for()))
        });
        let rows: Vec<LastSeasonRow> = ordered
            .into_iter()
            .enumerate()
            .map(|(i, r)| {
                let is_champ = champion == Some(r.roster_id);
                LastSeasonRow {
                    place: i as u32 + 1,
                    name: r
                        .owner_id
                        .as_ref()
                        .and_then(|o| names.get(o).cloned())
                        .unwrap_or_else(|| format!("Team {}", r.roster_id)),
                    record: if r.settings.ties > 0 {
                        format!(
                            "{}\u{2013}{}\u{2013}{}",
                            r.settings.wins, r.settings.losses, r.settings.ties
                        )
                    } else {
                        format!("{}\u{2013}{}", r.settings.wins, r.settings.losses)
                    },
                    points: r.settings.points_for(),
                    tag: if is_champ {
                        Some("Champ".into())
                    } else if most_points == Some(r.roster_id) {
                        Some("Most pts".into())
                    } else {
                        None
                    },
                    is_mine: my_user_id.is_some() && r.owner_id.as_deref() == my_user_id,
                }
            })
            .collect();
        self.write_cache(&name, &rows);
        rows
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
}

impl SeasonLoader for Engine {
    async fn load_season(
        &self,
        league: &League,
        my_user_id: Option<&str>,
        force: bool,
    ) -> Result<LoadedSeason, String> {
        let state = self.client.nfl_state().await?;
        let week = state.current_week();
        let season: u32 = league
            .season
            .parse()
            .map_err(|_| format!("league season '{}' is not a year", league.season))?;
        let league_id = league.league_id.as_str();
        let mut warnings = Vec::new();

        let (rosters, matchups, scores) = tokio::join!(
            self.client.rosters(league_id),
            self.client.matchups(league_id, week),
            self.client.nfl_scores(season, week)
        );
        let rosters = rosters?;
        // The same per-source bookkeeping the live poll keeps, so the health
        // badge starts out honest rather than waiting for the first refresh.
        let loaded_at = now_secs();
        let mut sources = SourceHealth::default();
        sources.rosters.succeeded(loaded_at);
        let matchups = match matchups {
            Ok(matchups) => {
                sources.matchups.succeeded(loaded_at);
                matchups
            }
            Err(error) => {
                warnings.push(format!("this week's matchups unavailable: {error}"));
                sources.matchups.failed(error);
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
                sources.scores.failed(error);
                Vec::new()
            }
        };

        // The activity feed spans this week and last, which is what "recent"
        // means to someone checking waivers.
        // In week 1 "last week" is week 1 too; fetching it twice would list
        // every preseason move twice.
        let mut weeks = vec![week.saturating_sub(1).max(1), week];
        weeks.dedup();
        let mut transactions: Vec<Transaction> = Vec::new();
        for w in weeks {
            match self.client.transactions(league_id, w).await {
                Ok(batch) => {
                    for t in batch {
                        if !transactions
                            .iter()
                            .any(|seen| seen.transaction_id == t.transaction_id)
                        {
                            transactions.push(t);
                        }
                    }
                }
                Err(error) => {
                    warnings.push(format!("transactions for week {w} unavailable: {error}"))
                }
            }
        }

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
            rosters,
            matchups,
            schedule: sweep.schedule,
            season_points: sweep.season_points,
            transactions,
            scores,
            last_season,
            history: History::default(),
            fetched_at: loaded_at,
            warnings,
            sources,
        })
    }

    /// Refresh only the fast-moving parts: this week's scoring and the NFL
    /// scoreboard. Used by the in-season poller.
    async fn refresh_live(&self, season: &mut LoadedSeason, league_id: &str) -> Result<(), String> {
        let (matchups, scores, rosters) = tokio::join!(
            self.client.matchups(league_id, season.week),
            self.client.nfl_scores(season.season, season.week),
            self.client.rosters(league_id)
        );
        // Which endpoint gave what, and what that means for the staleness
        // clock, is decided in `season_sources` where it can be tested without
        // a network.
        apply_refresh(season, matchups, scores, rosters, now_secs())
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::season_api::Matchup;

    fn matchup(roster_id: u32, matchup_id: Option<u32>) -> Matchup {
        Matchup {
            roster_id,
            matchup_id,
            points: 0.0,
            custom_points: None,
            starters: None,
            players: None,
            players_points: None,
        }
    }

    #[test]
    fn matchup_ids_pair_rosters_into_games() {
        let pairs = pairs_from(&[
            matchup(3, Some(1)),
            matchup(1, Some(1)),
            matchup(2, Some(2)),
            matchup(4, Some(2)),
        ]);
        assert_eq!(pairs, vec![(1, 3), (2, 4)]);
    }

    #[test]
    fn byes_and_unscheduled_rosters_produce_no_game() {
        // A lone roster on a matchup id, and one with no id at all.
        let pairs = pairs_from(&[matchup(1, Some(1)), matchup(2, None)]);
        assert!(pairs.is_empty());
    }
}
