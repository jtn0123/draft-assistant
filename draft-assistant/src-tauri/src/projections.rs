//! Cache-backed fetchers for the three big Sleeper payloads.
//!
//! Each follows the same policy: serve a fresh cache when there is one, fetch
//! otherwise, and on a network failure fall back to whatever stale copy is on
//! disk with a warning attached. A projections outage should degrade the
//! board's freshness, never take the app down mid-draft.

use crate::engine::{
    now_secs, Engine, PLAYERS_TTL_SECS, PROJECTIONS_TTL_SECS, REQUEST_CONCURRENCY, WEEKS,
};
use crate::sleeper::{PlayerMeta, ProjectionRow};
use crate::sleeper_error::to_message;
use futures_util::StreamExt;
use std::collections::HashMap;

impl Engine {
    /// The players dictionary, fetched and then parsed off the runtime.
    ///
    /// ~14.6 MB of JSON. Deserialising it in-task stopped every other task —
    /// including both pollers — for hundreds of milliseconds on every cold
    /// load. Only the disk read was moved off-thread before; this is the
    /// network path.
    async fn fetch_players(&self) -> Result<HashMap<String, PlayerMeta>, String> {
        let bytes = self.client.players_bytes().await.map_err(to_message)?;
        tokio::task::spawn_blocking(move || serde_json::from_slice(&bytes))
            .await
            .map_err(|e| format!("could not read the player list: {e}"))?
            .map_err(|e| format!("could not read the player list: {e}"))
    }

    pub(crate) async fn players(
        &self,
        force: bool,
    ) -> Result<(u64, HashMap<String, PlayerMeta>, Option<String>), String> {
        if !force {
            if let Some(hit) = self
                .read_cache_off_thread("players.json", PLAYERS_TTL_SECS)
                .await
            {
                return Ok((hit.0, hit.1, None));
            }
        }
        let stale = self.read_cache_any_off_thread("players.json").await;
        match self.fetch_players().await {
            Ok(data) => {
                let at = self.write_cache_off_thread("players.json", &data).await;
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
            if let Some(hit) = self
                .read_cache_off_thread(&name, PROJECTIONS_TTL_SECS)
                .await
            {
                return Ok((hit.0, hit.1, None));
            }
        }
        let stale = self.read_cache_any_off_thread(&name).await;
        match self
            .client
            .season_projections(season)
            .await
            .map_err(to_message)
        {
            Ok(data) => {
                let at = self.write_cache_off_thread(&name, &data).await;
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
            if let Some(hit) = self
                .read_cache_off_thread(&name, PROJECTIONS_TTL_SECS)
                .await
            {
                return Ok((hit.0, hit.1, None));
            }
        }
        let stale = self.read_cache_any_off_thread(&name).await;
        // Eighteen weeks, six at a time: sequentially this was eighteen round
        // trips end to end, and at an 8s timeout a bad connection turned a
        // league load into minutes of waiting.
        let fetched: Vec<(u32, Result<Vec<ProjectionRow>, String>)> =
            futures_util::stream::iter(1..=WEEKS)
                .map(|week| async move {
                    (
                        week,
                        self.client
                            .weekly_projections(season, week)
                            .await
                            .map_err(to_message),
                    )
                })
                .buffer_unordered(REQUEST_CONCURRENCY)
                .collect()
                .await;

        let mut all = Vec::new();
        let mut failures = Vec::new();
        for (week, result) in fetched {
            match result {
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
        all.sort_by_key(|w| w.week);
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
        if failures.is_empty() {
            let at = self.write_cache_off_thread(&name, &all).await;
            return Ok((at, all, None));
        }
        // A partial sweep is never written back. Stamped fresh it would serve
        // for the whole TTL, and a week missing from the file is a week with
        // no bonus expectation and no bye information at all — so the wrong
        // answer would stick around long after the outage that caused it.
        let warning = format!(
            "weekly projections unavailable for weeks {}",
            failures
                .iter()
                .map(u32::to_string)
                .collect::<Vec<_>>()
                .join(", ")
        );
        let at = match stale {
            Some((at, cached)) => {
                // Fill the holes from the copy on disk. Those weeks are older
                // than the rest, which is what `at` now says.
                all.extend(
                    cached
                        .into_iter()
                        .filter(|row| row.week.is_some_and(|w| failures.contains(&w))),
                );
                all.sort_by_key(|w| w.week);
                at
            }
            None => now_secs(),
        };
        Ok((at, all, Some(warning)))
    }
}

/// Shared by cache-policy tests here and in `season_engine`: an engine whose
/// HTTP always fails instantly without touching the real network.
#[cfg(test)]
pub(crate) mod test_support {
    use crate::engine::{now_secs, Engine};
    use std::sync::Once;

    static OFFLINE: Once = Once::new();

    /// Point all HTTP at a proxy on a closed local port: the listener is
    /// bound only to reserve a port nothing listens on, then dropped, so
    /// every request gets connection-refused immediately.
    pub(crate) fn offline_engine(label: &str) -> Engine {
        OFFLINE.call_once(|| {
            let port = std::net::TcpListener::bind("127.0.0.1:0")
                .and_then(|l| l.local_addr())
                .map(|a| a.port())
                .unwrap_or(9);
            let proxy = format!("http://127.0.0.1:{port}");
            std::env::set_var("HTTP_PROXY", &proxy);
            std::env::set_var("HTTPS_PROXY", &proxy);
        });
        let dir = std::env::temp_dir().join(format!(
            "draft-assistant-cache-{label}-{}-{}",
            std::process::id(),
            now_secs()
        ));
        Engine::new(dir)
    }
}

#[cfg(test)]
mod tests {
    use super::test_support::offline_engine;
    use crate::sleeper::{PlayerMeta, ProjectionRow};
    use std::collections::HashMap;

    fn players_fixture() -> HashMap<String, PlayerMeta> {
        serde_json::from_str(r#"{"p1": {"full_name": "Cache Hit", "position": "RB"}}"#).unwrap()
    }

    fn rows_fixture() -> Vec<ProjectionRow> {
        serde_json::from_str(r#"[{"player_id": "p1", "stats": {"adp_ppr": 12.0}}]"#).unwrap()
    }

    #[tokio::test]
    async fn players_serves_a_fresh_cache_without_fetching() {
        let engine = offline_engine("players-fresh");
        engine.write_cache("players.json", &players_fixture());
        let (at, data, warning) = engine.players(false).await.unwrap();
        assert!(at > 0);
        assert_eq!(data["p1"].full_name.as_deref(), Some("Cache Hit"));
        assert!(warning.is_none(), "cache hit should carry no warning");
        std::fs::remove_dir_all(engine.data_dir).unwrap();
    }

    #[tokio::test]
    async fn players_outage_falls_back_to_the_stale_cache_with_a_warning() {
        let engine = offline_engine("players-stale");
        engine.write_cache("players.json", &players_fixture());
        // force=true skips the fresh cache and hits the (dead) network.
        let (_, data, warning) = engine.players(true).await.unwrap();
        assert!(data.contains_key("p1"));
        let warning = warning.expect("stale fallback must warn");
        assert!(warning.contains("players refresh failed"), "{warning}");
        assert!(warning.contains("using cache aged"), "{warning}");
        std::fs::remove_dir_all(engine.data_dir).unwrap();
    }

    #[tokio::test]
    async fn players_outage_with_no_cache_is_an_error() {
        let engine = offline_engine("players-none");
        let err = engine.players(false).await.unwrap_err();
        assert!(err.contains("request failed"), "{err}");
        std::fs::remove_dir_all(engine.data_dir).unwrap();
    }

    #[tokio::test]
    async fn season_projections_cache_is_keyed_by_season() {
        let engine = offline_engine("season-fresh");
        engine.write_cache("projections_2025.json", &rows_fixture());
        let (_, data, warning) = engine.season_projections(2025, false).await.unwrap();
        assert_eq!(data[0].player_id, "p1");
        assert_eq!(data[0].stat("adp_ppr"), Some(12.0));
        assert!(warning.is_none());
        // A different season misses this cache and fails offline.
        assert!(engine.season_projections(2024, false).await.is_err());
        std::fs::remove_dir_all(engine.data_dir).unwrap();
    }

    #[tokio::test]
    async fn season_projections_outage_falls_back_to_the_stale_cache() {
        let engine = offline_engine("season-stale");
        engine.write_cache("projections_2025.json", &rows_fixture());
        let (_, data, warning) = engine.season_projections(2025, true).await.unwrap();
        assert_eq!(data.len(), 1);
        let warning = warning.expect("stale fallback must warn");
        assert!(warning.contains("projections refresh failed"), "{warning}");
        std::fs::remove_dir_all(engine.data_dir).unwrap();
    }

    #[tokio::test]
    async fn weekly_projections_serve_a_fresh_cache_without_fetching() {
        let engine = offline_engine("weekly-fresh");
        engine.write_cache("weekly_2025.json", &rows_fixture());
        let (_, data, warning) = engine.weekly_projections(2025, false).await.unwrap();
        assert_eq!(data.len(), 1);
        assert!(warning.is_none());
        std::fs::remove_dir_all(engine.data_dir).unwrap();
    }

    #[tokio::test]
    async fn weekly_projections_total_outage_falls_back_to_the_stale_cache() {
        let engine = offline_engine("weekly-stale");
        engine.write_cache("weekly_2025.json", &rows_fixture());
        // force=true makes all 18 week fetches fail, which must not clobber
        // the cached copy with an empty result.
        let (_, data, warning) = engine.weekly_projections(2025, true).await.unwrap();
        assert_eq!(data.len(), 1);
        let warning = warning.expect("stale fallback must warn");
        assert!(
            warning.contains("weekly projections refresh failed"),
            "{warning}"
        );
        std::fs::remove_dir_all(engine.data_dir).unwrap();
    }

    #[tokio::test]
    async fn weekly_projections_total_outage_with_no_cache_is_an_error() {
        let engine = offline_engine("weekly-none");
        let err = engine.weekly_projections(2025, false).await.unwrap_err();
        assert_eq!(err, "all weekly projection requests failed");
        std::fs::remove_dir_all(engine.data_dir).unwrap();
    }
}
