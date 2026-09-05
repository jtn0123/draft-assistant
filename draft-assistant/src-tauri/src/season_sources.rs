//! Per-source freshness for the live refresh.
//!
//! Each poll pulls three independent things, and a partial success still
//! counts as fresh — one dead endpoint should not blank the screen. The cost
//! of that is a single overall "fetched at" stamp that can stay green while
//! one source has been failing for an hour. So each source also keeps its own
//! last-success stamp and its own last error, and the badge can say precisely
//! which part of the screen it no longer stands behind.

use crate::season_api::{Matchup, Roster, ScoreGame};
use crate::season_engine::LoadedSeason;
use serde::{Deserialize, Serialize};

/// One upstream feed's standing: when it last worked, and why the most recent
/// attempt did not.
#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct SourceStatus {
    /// Epoch seconds of the last successful fetch. Zero when it has never
    /// answered since the season was loaded.
    #[serde(default)]
    pub last_success_secs: u64,
    /// Why the latest attempt failed; `None` when the latest attempt worked.
    #[serde(default)]
    pub error: Option<String>,
}

impl SourceStatus {
    /// A fresh answer: stamp it and drop any earlier complaint.
    pub fn succeeded(&mut self, now: u64) {
        self.last_success_secs = now;
        self.error = None;
    }

    /// A failed attempt: keep the reason, leave the last-success stamp where
    /// it was. The age of the data is the point of this record.
    pub fn failed(&mut self, error: String) {
        self.error = Some(error);
    }
}

/// The three feeds the live poll depends on, tracked one by one.
#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct SourceHealth {
    #[serde(default)]
    pub matchups: SourceStatus,
    #[serde(default)]
    pub scores: SourceStatus,
    #[serde(default)]
    pub rosters: SourceStatus,
}

/// One round of live fetches, before anything has been locked to apply them.
///
/// Fetching and applying are separate steps on purpose. Three requests at an
/// eight-second timeout with three retries apiece is tens of seconds in the
/// worst case, and the `season` mutex used to be held for all of it — which
/// stopped `get_season`, `load_season` and every chat question behind it, and
/// queued the next poll tick behind the current one. Now the fetch runs with
/// nothing locked and only the fold below takes the lock.
pub struct LiveFetch {
    pub matchups: Result<Vec<Matchup>, String>,
    pub scores: Result<Vec<ScoreGame>, String>,
    pub rosters: Result<Vec<Roster>, String>,
}

impl LiveFetch {
    /// Fold this round into the season. The short half: no network, no waiting.
    pub fn apply(self, season: &mut LoadedSeason, now: u64) -> Result<(), String> {
        apply_refresh(season, self.matchups, self.scores, self.rosters, now)
    }
}

/// Fold one round of live fetches into the season.
///
/// Every source that answered is applied and stamped; every source that failed
/// leaves its previous data and last-success stamp alone and records the
/// reason. The overall `fetched_at` advances whenever anything arrived,
/// because the screen really did get newer — `Err` only when nothing did, so
/// the staleness clock does not reset on data that never came.
fn apply_refresh(
    season: &mut LoadedSeason,
    matchups: Result<Vec<Matchup>, String>,
    scores: Result<Vec<ScoreGame>, String>,
    rosters: Result<Vec<Roster>, String>,
    now: u64,
) -> Result<(), String> {
    let mut errors = Vec::new();
    match matchups {
        Ok(value) => {
            season.matchups = std::sync::Arc::new(value);
            season.sources.matchups.succeeded(now);
        }
        Err(error) => {
            errors.push(format!("matchups: {error}"));
            season.sources.matchups.failed(error);
        }
    }
    match scores {
        Ok(value) => {
            season.scores = std::sync::Arc::new(value);
            season.sources.scores.succeeded(now);
        }
        Err(error) => {
            errors.push(format!("scores: {error}"));
            season.sources.scores.failed(error);
        }
    }
    match rosters {
        Ok(value) => {
            season.rosters = std::sync::Arc::new(value);
            season.sources.rosters.succeeded(now);
        }
        Err(error) => {
            errors.push(format!("rosters: {error}"));
            season.sources.rosters.failed(error);
        }
    }
    if errors.len() == 3 {
        return Err(errors.join("; "));
    }
    season.fetched_at = now;
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    /// A season that has just loaded cleanly: all three sources stamped.
    fn loaded_at(now: u64) -> LoadedSeason {
        let mut season = LoadedSeason::default();
        apply_refresh(
            &mut season,
            Ok(Vec::new()),
            Ok(Vec::new()),
            Ok(Vec::new()),
            now,
        )
        .expect("a clean load");
        season
    }

    #[test]
    fn one_source_failing_for_an_hour_does_not_hide_behind_the_others() {
        let mut season = loaded_at(1_000);
        // Two minutes of polls: rosters times out every time, the other two
        // answer every time.
        for now in [1_030, 1_060, 1_090, 1_120] {
            apply_refresh(
                &mut season,
                Ok(Vec::new()),
                Ok(Vec::new()),
                Err("timeout".into()),
                now,
            )
            .expect("two of three answered, so the refresh succeeded");
        }

        // The healthy sources are current and quiet.
        assert_eq!(season.sources.matchups.last_success_secs, 1_120);
        assert_eq!(season.sources.matchups.error, None);
        assert_eq!(season.sources.scores.last_success_secs, 1_120);
        assert_eq!(season.sources.scores.error, None);

        // Rosters still carries the reason and the age of the data it holds.
        assert_eq!(season.sources.rosters.last_success_secs, 1_000);
        assert_eq!(season.sources.rosters.error.as_deref(), Some("timeout"));

        // And the overall stamp still advances: most of the screen is fresh.
        assert_eq!(season.fetched_at, 1_120);
    }

    #[test]
    fn a_source_that_comes_back_stops_complaining() {
        let mut season = loaded_at(1_000);
        apply_refresh(
            &mut season,
            Err("503".into()),
            Ok(Vec::new()),
            Ok(Vec::new()),
            1_030,
        )
        .expect("two of three answered");
        assert_eq!(season.sources.matchups.error.as_deref(), Some("503"));

        apply_refresh(
            &mut season,
            Ok(Vec::new()),
            Ok(Vec::new()),
            Ok(Vec::new()),
            1_060,
        )
        .expect("everything answered");
        assert_eq!(season.sources.matchups.error, None);
        assert_eq!(season.sources.matchups.last_success_secs, 1_060);
    }

    #[test]
    fn a_total_outage_names_every_source_and_freezes_the_clock() {
        let mut season = loaded_at(1_000);
        let error = apply_refresh(
            &mut season,
            Err("a".into()),
            Err("b".into()),
            Err("c".into()),
            2_000,
        )
        .expect_err("nothing arrived, so nothing is fresh");
        for source in ["matchups", "scores", "rosters"] {
            assert!(error.contains(source), "{source} missing from: {error}");
        }
        assert_eq!(
            season.fetched_at, 1_000,
            "the staleness clock must not move"
        );
        assert_eq!(season.sources.rosters.last_success_secs, 1_000);
    }
}
