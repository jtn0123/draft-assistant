//! The full-season matchup sweep: one request per regular-season week, each
//! week cached on its own so a rollover costs one request instead of fifteen.
//!
//! Split out of `season_engine.rs`, which was at the line cap.

use super::rows::pairs_from;
use super::WeekPairings;
use crate::engine::{Engine, REQUEST_CONCURRENCY};
use crate::season_api::{Matchup, SeasonEndpoints};
use crate::sleeper_error::to_message;
use futures_util::StreamExt;
use std::collections::HashMap;

const WEEK_SWEEP_TTL_SECS: u64 = 6 * 3600;
/// What a matchup week says when Sleeper answered it with nothing.
pub const EMPTY_WEEK: &str = "came back with no matchup rows";

/// The full-season matchup sweep, assembled from the per-week caches.
pub(super) struct WeekSweep {
    pub(super) schedule: WeekPairings,
    pub(super) season_points: HashMap<String, f64>,
}

impl Engine {
    /// One week's matchup rows, cached on their own.
    ///
    /// A week that is already over can never change again, so it is kept
    /// forever; the current week and the ones still to come keep the old
    /// six-hour TTL. That is what makes a weekly rollover cost one request
    /// instead of fifteen — the sweep used to be a single blob stamped with
    /// the week it was taken in, so the week ticking over threw all of it
    /// away.
    pub(super) async fn week_matchups(
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
            if let Some((_, matchups)) =
                self.read_cache_off_thread::<Vec<Matchup>>(&name, ttl).await
            {
                return Ok(matchups);
            }
        }
        let matchups = self
            .client
            .matchups(league_id, week)
            .await
            .map_err(to_message)?;
        // Sleeper answers `null` now and then, which parses as no rows. A
        // finished week always has rows, so an empty answer for one is a lost
        // response and is reported as such; an empty answer for the week
        // being played or a later one is passed on but never written to
        // disk. The settled-week cache is read back at `ttl = u64::MAX`, so
        // one blank answer written there would have stood as that week's
        // result for the rest of the season.
        if matchups.is_empty() {
            if settled {
                return Err(format!("week {week} {EMPTY_WEEK}"));
            }
            return Ok(matchups);
        }
        self.write_season_cache(&name, &matchups).await;
        Ok(matchups)
    }

    /// Sweep every regular-season week: pairings for the simulation and
    /// season-to-date points per player. Weeks already on disk cost nothing.
    pub(super) async fn week_sweep(
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
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::engine::now_secs;
    use crate::projections::test_support::{counting_stub, json_response, offline_engine};
    use crate::season_engine::rows::matchup;
    use crate::sleeper::SleeperClient;
    use std::sync::atomic::Ordering;

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

    /// The guard on the settled-week cache, tested without it: Sleeper's
    /// occasional `null` parses as no rows, a finished week always has rows,
    /// and a blank answer written under a `u64::MAX` TTL would have stood as
    /// that week's result for the rest of the season.
    #[tokio::test]
    async fn an_empty_answer_for_a_finished_week_is_refused_and_never_cached() {
        let (host, hits) = counting_stub(json_response("[]"));
        let dir = std::env::temp_dir().join(format!(
            "draft-assistant-empty-week-{}-{}",
            std::process::id(),
            now_secs()
        ));
        let engine = Engine::with_client(dir.clone(), SleeperClient::with_host(host));

        let error = engine
            .week_matchups("league-1", 3, 5, false)
            .await
            .expect_err("a finished week with no rows is a lost response");
        assert!(error.contains(EMPTY_WEEK), "{error}");
        assert_eq!(hits.load(Ordering::SeqCst), 1, "the request was made");
        let settled = Engine::season_cache_name("league-1", "week3");
        assert!(
            !dir.join(&settled).exists(),
            "an empty settled week was written to the forever cache"
        );

        // The week being played may legitimately be empty (nothing has been
        // scheduled yet), so it is passed on but still not written down.
        let current = engine
            .week_matchups("league-1", 5, 5, false)
            .await
            .expect("an empty current week is not an error");
        assert!(current.is_empty());
        assert!(
            !dir.join(Engine::season_cache_name("league-1", "week5"))
                .exists(),
            "an empty current week was cached"
        );
        // And a refetch goes back to the network rather than to a blank file.
        engine.week_matchups("league-1", 3, 5, false).await.ok();
        assert_eq!(hits.load(Ordering::SeqCst), 3);
        std::fs::remove_dir_all(dir).ok();
    }
}
