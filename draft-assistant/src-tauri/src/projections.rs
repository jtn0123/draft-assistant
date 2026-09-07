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

    /// The players dictionary, no older than the usual 24 hours (`force`
    /// skips the cache altogether).
    pub(crate) async fn players(
        &self,
        force: bool,
    ) -> Result<(u64, HashMap<String, PlayerMeta>, Option<String>), String> {
        self.players_no_older_than((!force).then_some(PLAYERS_TTL_SECS))
            .await
    }

    /// The players dictionary, served from the cache only while the cached
    /// copy is at most `max_age` seconds old; `None` skips the cache.
    ///
    /// The 24-hour default is right for a draft board and wrong for an app
    /// left open on a Sunday: the season poller's half-hour injury refresh
    /// asked with the default, was handed the morning's cache back every
    /// time, and a starter ruled Out at noon stayed in the optimal lineup
    /// all day. The poller asks with its own half-hour ceiling instead.
    pub(crate) async fn players_no_older_than(
        &self,
        max_age: Option<u64>,
    ) -> Result<(u64, HashMap<String, PlayerMeta>, Option<String>), String> {
        if let Some(max_age) = max_age {
            if let Some(hit) = self.read_cache_off_thread("players.json", max_age).await {
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

    /// Every week's projections, no older than the usual six hours (`force`
    /// skips the cache altogether).
    pub(crate) async fn weekly_projections(
        &self,
        season: u32,
        force: bool,
    ) -> Result<(u64, Vec<ProjectionRow>, Option<String>), String> {
        self.weekly_projections_no_older_than(season, (!force).then_some(PROJECTIONS_TTL_SECS))
            .await
    }

    /// Every week's projections, served from the cache only while the cached
    /// copy is at most `max_age` seconds old; `None` skips the cache. See
    /// [`Engine::players_no_older_than`] for why the caller chooses.
    pub(crate) async fn weekly_projections_no_older_than(
        &self,
        season: u32,
        max_age: Option<u64>,
    ) -> Result<(u64, Vec<ProjectionRow>, Option<String>), String> {
        let name = format!("weekly_{season}.json");
        if let Some(max_age) = max_age {
            if let Some(hit) = self.read_cache_off_thread(&name, max_age).await {
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
                    crate::applog::warn(format!("weekly projections week {week} failed: {e}"));
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
    use crate::sleeper::SleeperClient;
    use std::io::{Read, Write};
    use std::net::TcpListener;
    use std::sync::atomic::{AtomicUsize, Ordering};
    use std::sync::Arc;

    /// Port 1 is reserved, needs root to bind, and nothing serves it, so a
    /// connection there is refused immediately. Proxies are ignored by
    /// `with_host`, so this is the whole story about where these requests go.
    pub(crate) const DEAD_HOST: &str = "http://127.0.0.1:1";

    /// An engine pointed at [`DEAD_HOST`]. Per client, not per process: the
    /// proxy variables this used to set were shared with every other thread
    /// in the test binary, which is a race and leaked into tests that wanted
    /// a real localhost server.
    pub(crate) fn offline_engine(label: &str) -> Engine {
        let dir = std::env::temp_dir().join(format!(
            "draft-assistant-cache-{label}-{}-{}",
            std::process::id(),
            now_secs()
        ));
        Engine::with_client(dir, SleeperClient::with_host(DEAD_HOST))
    }

    /// A complete HTTP response carrying `body` as JSON, so a test never
    /// has to count bytes by hand.
    pub(crate) fn json_response(body: &str) -> String {
        format!(
            "HTTP/1.1 200 OK\r\nContent-Type: application/json\r\nContent-Length: {}\r\nConnection: close\r\n\r\n{body}",
            body.len()
        )
    }

    /// A server that answers every request with `response` verbatim and
    /// counts the connections it accepted. `Connection: close` in the
    /// response keeps the client from pooling, so one request is exactly one
    /// count.
    pub(crate) fn counting_stub(response: String) -> (String, Arc<AtomicUsize>) {
        let listener = TcpListener::bind("127.0.0.1:0").expect("bind a stub server");
        let host = format!("http://{}", listener.local_addr().unwrap());
        let hits = Arc::new(AtomicUsize::new(0));
        let counter = Arc::clone(&hits);
        std::thread::spawn(move || {
            for stream in listener.incoming() {
                let Ok(mut stream) = stream else { break };
                counter.fetch_add(1, Ordering::SeqCst);
                let mut buffer = [0u8; 4096];
                let _ = stream.read(&mut buffer);
                let _ = stream.write_all(response.as_bytes());
                let _ = stream.flush();
            }
        });
        (host, hits)
    }
}

#[cfg(test)]
mod tests {
    use super::test_support::{counting_stub, json_response, offline_engine};
    use crate::engine::Engine;
    use crate::sleeper::{PlayerMeta, ProjectionRow, SleeperClient};
    use std::collections::HashMap;
    use std::sync::atomic::Ordering;

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

    /// Write a players cache stamped `age` seconds ago.
    fn aged_players_cache(engine: &Engine, age: u64) {
        let path = engine.data_dir.join("players.json");
        std::fs::create_dir_all(&engine.data_dir).unwrap();
        crate::cache::write_atomic(
            engine.data_dir.join("players.json.tmp"),
            path,
            crate::engine::now_secs() - age,
            &players_fixture(),
        )
        .unwrap();
    }

    /// The bug: the season poller's half-hour refresh asked for the players
    /// with the 24-hour default, so the morning's cache came back every time
    /// and a starter ruled Out at noon stayed in the lineup all day.
    #[tokio::test]
    async fn a_half_hour_ceiling_refetches_a_cache_that_is_older_than_that() {
        let (host, hits) = counting_stub(json_response(
            r#"{"p2": {"full_name": "Fresh", "position": "RB"}}"#,
        ));
        let dir = std::env::temp_dir().join(format!(
            "draft-assistant-players-ceiling-{}-{}",
            std::process::id(),
            crate::engine::now_secs()
        ));
        let engine = Engine::with_client(dir, SleeperClient::with_host(host));

        // Thirty-one minutes old: inside the 24h default, outside the ceiling.
        aged_players_cache(&engine, 31 * 60);
        let (_, data, _) = engine.players(false).await.unwrap();
        assert!(
            data.contains_key("p1"),
            "the default still serves a day-old cache"
        );
        assert_eq!(hits.load(Ordering::SeqCst), 0, "the default must not fetch");

        let (_, data, warning) = engine.players_no_older_than(Some(30 * 60)).await.unwrap();
        assert_eq!(
            hits.load(Ordering::SeqCst),
            1,
            "a 31-minute cache is past a 30-minute ceiling"
        );
        assert!(
            data.contains_key("p2"),
            "the fetched copy is the one handed back"
        );
        assert!(warning.is_none());

        // Five minutes old: served from disk, no request.
        aged_players_cache(&engine, 5 * 60);
        let (_, data, _) = engine.players_no_older_than(Some(30 * 60)).await.unwrap();
        assert!(data.contains_key("p1"));
        assert_eq!(
            hits.load(Ordering::SeqCst),
            1,
            "a five-minute cache must not be refetched"
        );
        std::fs::remove_dir_all(engine.data_dir).unwrap();
    }

    #[tokio::test]
    async fn the_weekly_ceiling_refetches_an_old_cache_and_keeps_a_young_one() {
        let engine = offline_engine("weekly-ceiling");
        std::fs::create_dir_all(&engine.data_dir).unwrap();
        crate::cache::write_atomic(
            engine.data_dir.join("weekly_2025.json.tmp"),
            engine.data_dir.join("weekly_2025.json"),
            crate::engine::now_secs() - 31 * 60,
            &rows_fixture(),
        )
        .unwrap();
        // Offline, a refetch fails and falls back to the stale copy with a
        // warning: the warning is the proof that the network was asked.
        let (_, _, warning) = engine
            .weekly_projections_no_older_than(2025, Some(30 * 60))
            .await
            .unwrap();
        assert!(warning.is_some(), "a 31-minute cache must be refetched");
        let (_, _, warning) = engine
            .weekly_projections_no_older_than(2025, Some(40 * 60))
            .await
            .unwrap();
        assert!(
            warning.is_none(),
            "a cache inside the ceiling is served as is"
        );
        std::fs::remove_dir_all(engine.data_dir).unwrap();
    }
}
