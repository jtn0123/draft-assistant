//! Cache-backed fetchers for the three big Sleeper payloads.
//!
//! Each follows the same policy: serve a fresh cache when there is one, fetch
//! otherwise, and on a network failure fall back to whatever stale copy is on
//! disk with a warning attached. A projections outage should degrade the
//! board's freshness, never take the app down mid-draft.

use crate::engine::{now_secs, Engine, PLAYERS_TTL_SECS, PROJECTIONS_TTL_SECS, WEEKS};
use crate::sleeper::{PlayerMeta, ProjectionRow};
use std::collections::HashMap;

impl Engine {
    pub(crate) async fn players(
        &self,
        force: bool,
    ) -> Result<(u64, HashMap<String, PlayerMeta>, Option<String>), String> {
        if !force {
            if let Some(hit) = self.read_cache("players.json", PLAYERS_TTL_SECS) {
                return Ok((hit.0, hit.1, None));
            }
        }
        let stale = self.read_cache_any("players.json");
        match self.client.players().await {
            Ok(data) => {
                let at = self.write_cache("players.json", &data);
                Ok((at, data, None))
            }
            Err(error) => stale
                .map(|(at, data)| {
                    let age = now_secs().saturating_sub(at);
                    (
                        at,
                        data,
                        Some(format!(
                            "players refresh failed; using cache aged {}h ({error})",
                            age / 3600
                        )),
                    )
                })
                .ok_or(error),
        }
    }

    pub(crate) async fn season_projections(
        &self,
        season: u32,
        force: bool,
    ) -> Result<(u64, Vec<ProjectionRow>, Option<String>), String> {
        let name = format!("projections_{season}.json");
        if !force {
            if let Some(hit) = self.read_cache(&name, PROJECTIONS_TTL_SECS) {
                return Ok((hit.0, hit.1, None));
            }
        }
        let stale = self.read_cache_any(&name);
        match self.client.season_projections(season).await {
            Ok(data) => {
                let at = self.write_cache(&name, &data);
                Ok((at, data, None))
            }
            Err(error) => stale
                .map(|(at, data)| {
                    let age = now_secs().saturating_sub(at);
                    (
                        at,
                        data,
                        Some(format!(
                            "projections refresh failed; using cache aged {}h ({error})",
                            age / 3600
                        )),
                    )
                })
                .ok_or(error),
        }
    }

    pub(crate) async fn weekly_projections(
        &self,
        season: u32,
        force: bool,
    ) -> Result<(u64, Vec<ProjectionRow>, Option<String>), String> {
        let name = format!("weekly_{season}.json");
        if !force {
            if let Some(hit) = self.read_cache(&name, PROJECTIONS_TTL_SECS) {
                return Ok((hit.0, hit.1, None));
            }
        }
        let stale = self.read_cache_any(&name);
        let mut all = Vec::new();
        let mut failures = Vec::new();
        for week in 1..=WEEKS {
            match self.client.weekly_projections(season, week).await {
                Ok(mut rows) => {
                    for r in &mut rows {
                        r.week = Some(week);
                    }
                    all.extend(rows);
                }
                Err(e) => {
                    // A missing week degrades bonus precision, not correctness.
                    eprintln!("weekly projections week {week} failed: {e}");
                    failures.push(week);
                }
            }
        }
        if failures.len() == WEEKS as usize {
            let error = "all weekly projection requests failed".to_string();
            return stale
                .map(|(at, data)| {
                    let age = now_secs().saturating_sub(at);
                    (
                        at,
                        data,
                        Some(format!(
                            "weekly projections refresh failed; using cache aged {}h",
                            age / 3600
                        )),
                    )
                })
                .ok_or(error);
        }
        let at = self.write_cache(&name, &all);
        let warning = if failures.is_empty() {
            None
        } else {
            Some(format!(
                "weekly projections unavailable for weeks {}",
                failures
                    .iter()
                    .map(u32::to_string)
                    .collect::<Vec<_>>()
                    .join(", ")
            ))
        };
        Ok((at, all, warning))
    }
}
