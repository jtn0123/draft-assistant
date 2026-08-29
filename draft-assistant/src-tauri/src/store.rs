//! On-disk persistence for the engine: the cache envelope, the projection and
//! player caches, manual picks, and the app config.
//!
//! Split out of `engine.rs` to keep that file focused on assembling a league.
//! Failure policy differs by artifact and is documented per method: cache
//! writes degrade to a warning, config and manual-pick writes are fatal.

use crate::engine::now_secs;
use crate::engine::{AppConfig, Cached, Engine};
use crate::sleeper::Pick;
use serde::Serialize;
use std::collections::HashSet;
use std::path::PathBuf;
use std::sync::atomic::{AtomicU64, Ordering};

/// Per-process counter so every write gets its own scratch file. Two loads can
/// overlap — React's dev-mode double mount, or Refresh data clicked during
/// launch — and with one shared `name.tmp` the loser's rename found nothing to
/// move and the weekly file was reported as "could not be cached".
static TMP_SEQ: AtomicU64 = AtomicU64::new(0);

impl Engine {
    pub(crate) fn cache_path(&self, name: &str) -> PathBuf {
        self.data_dir.join(name)
    }

    pub(crate) fn tmp_path(&self, name: &str) -> PathBuf {
        let seq = TMP_SEQ.fetch_add(1, Ordering::Relaxed);
        self.cache_path(&format!("{name}.{}.{seq}.tmp", std::process::id()))
    }

    /// A cache read that respects the TTL. Logged either way: "why did it
    /// re-download 20 MB on the venue wifi" is a question with an answer.
    pub(crate) fn read_cache<T: serde::de::DeserializeOwned>(
        &self,
        name: &str,
        ttl: u64,
    ) -> Option<(u64, T)> {
        let Some((fetched_at, data)) = self.read_cache_any(name) else {
            crate::log::info(format!("cache {name}: absent, fetching"));
            return None;
        };
        let age = now_secs().saturating_sub(fetched_at);
        if age > ttl {
            crate::log::info(format!(
                "cache {name}: expired ({age}s old, ttl {ttl}s), refetching"
            ));
            return None;
        }
        crate::log::info(format!("cache {name}: fresh ({age}s old)"));
        Some((fetched_at, data))
    }

    pub(crate) fn read_cache_any<T: serde::de::DeserializeOwned>(
        &self,
        name: &str,
    ) -> Option<(u64, T)> {
        let raw = std::fs::read_to_string(self.cache_path(name)).ok()?;
        let cached: Cached<T> = serde_json::from_str(&raw).ok()?;
        Some((cached.fetched_at, cached.data))
    }

    /// Cache writes are not fatal — the data is already in hand and can be
    /// refetched — but a silent failure means every later launch re-downloads,
    /// so the failure is returned as a warning for `DataHealth`.
    pub(crate) fn write_cache<T: Serialize>(&self, name: &str, data: &T) -> (u64, Option<String>) {
        match self.write_cache_checked(name, data) {
            Ok(fetched_at) => (fetched_at, None),
            Err(e) => (
                now_secs(),
                Some(format!("{name} could not be cached ({e}); will refetch")),
            ),
        }
    }

    pub(crate) fn write_cache_checked<T: Serialize>(
        &self,
        name: &str,
        data: &T,
    ) -> Result<u64, String> {
        let fetched_at = now_secs();
        let env = Cached { fetched_at, data };
        let json = serde_json::to_string(&env).map_err(|e| format!("serialize {name}: {e}"))?;
        let bytes = json.len();
        let tmp = self.tmp_path(name);
        std::fs::write(&tmp, json).map_err(|e| format!("write {}: {e}", tmp.display()))?;
        std::fs::rename(&tmp, self.cache_path(name)).map_err(|e| format!("replace {name}: {e}"))?;
        crate::log::info(format!("cache {name}: wrote {bytes} bytes"));
        Ok(fetched_at)
    }

    pub(crate) fn manual_picks_cache_name(draft_id: &str) -> String {
        let safe_id: String = draft_id
            .chars()
            .filter(|c| c.is_ascii_alphanumeric() || *c == '-' || *c == '_')
            .collect();
        format!("manual_picks_{safe_id}.json")
    }

    pub fn load_manual_picks(&self, draft_id: &str) -> Vec<Pick> {
        self.read_cache_any(&Self::manual_picks_cache_name(draft_id))
            .map(|(_, picks)| picks)
            .unwrap_or_default()
    }

    pub fn save_manual_picks(&self, draft_id: &str, picks: &[Pick]) -> Result<(), String> {
        self.write_cache_checked(&Self::manual_picks_cache_name(draft_id), &picks)?;
        Ok(())
    }

    /// Keeper pick numbers remembered for a draft. Sleeper's own flag misses
    /// some, and the position-based judgement only works before the draft
    /// reaches them: once it has, only this file knows.
    pub fn load_keepers(&self, draft_id: &str) -> HashSet<u32> {
        let name = Self::manual_picks_cache_name(draft_id).replace("manual_picks_", "keepers_");
        self.read_cache_any::<Vec<u32>>(&name)
            .map(|(_, picks)| picks.into_iter().collect())
            .unwrap_or_default()
    }

    pub fn save_keepers(&self, draft_id: &str, keepers: &HashSet<u32>) -> Result<(), String> {
        let name = Self::manual_picks_cache_name(draft_id).replace("manual_picks_", "keepers_");
        let mut sorted: Vec<u32> = keepers.iter().copied().collect();
        sorted.sort_unstable();
        self.write_cache_checked(&name, &sorted)?;
        Ok(())
    }

    pub fn load_config(&self) -> AppConfig {
        std::fs::read_to_string(self.cache_path("config.json"))
            .ok()
            .and_then(|s| serde_json::from_str(&s).ok())
            .unwrap_or_default()
    }

    /// Fatal on failure: the config holds the active league and user id, so a
    /// silent loss here sends the user back to the Setup screen on next launch
    /// with no explanation. Written via tmp+rename so a crash mid-write cannot
    /// leave a truncated config behind.
    pub fn save_config(&self, config: &AppConfig) -> Result<(), String> {
        let json =
            serde_json::to_string_pretty(config).map_err(|e| format!("serialize config: {e}"))?;
        let tmp = self.tmp_path("config.json");
        std::fs::write(&tmp, json).map_err(|e| format!("write {}: {e}", tmp.display()))?;
        std::fs::rename(&tmp, self.cache_path("config.json"))
            .map_err(|e| format!("replace config.json: {e}"))?;
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::engine::now_secs;

    fn test_dir(label: &str) -> PathBuf {
        let unique = format!(
            "draft-assistant-{label}-{}-{}",
            std::process::id(),
            now_secs()
        );
        std::env::temp_dir().join(unique)
    }

    #[test]
    fn expired_cache_is_still_available_for_outage_fallback() {
        let dir = test_dir("stale-cache");
        let engine = Engine::new(dir.clone()).expect("temp data dir");
        let cached = Cached {
            fetched_at: 1,
            data: vec![10_u32, 20_u32],
        };
        std::fs::write(
            engine.cache_path("test.json"),
            serde_json::to_string(&cached).unwrap(),
        )
        .unwrap();

        assert!(engine.read_cache::<Vec<u32>>("test.json", 1).is_none());
        assert_eq!(
            engine.read_cache_any::<Vec<u32>>("test.json"),
            Some((1, vec![10, 20]))
        );

        std::fs::remove_dir_all(dir).unwrap();
    }

    #[test]
    fn config_save_failure_is_reported_not_swallowed() {
        let dir = test_dir("config-failure");
        let engine = Engine::new(dir.clone()).expect("temp data dir");
        let config = AppConfig {
            my_user_id: Some("u1".into()),
            active_league_id: Some("l1".into()),
            leagues: Vec::new(),
        };

        engine.save_config(&config).expect("writable dir saves");
        assert_eq!(engine.load_config().active_league_id.as_deref(), Some("l1"));

        // Removing the directory makes the write fail; the command must learn
        // about it rather than reporting success and losing the league.
        std::fs::remove_dir_all(&dir).unwrap();
        let err = engine
            .save_config(&config)
            .expect_err("missing data dir must surface as an error");
        assert!(err.contains("config"), "{err}");
    }

    #[test]
    fn overlapping_writes_of_one_cache_all_succeed() {
        // The bug this guards: two loads racing on `weekly_2026.json` shared a
        // single `.tmp`, so the second rename failed with "No such file".
        let dir = test_dir("cache-race");
        let engine = std::sync::Arc::new(Engine::new(dir.clone()).expect("temp data dir"));
        let workers: Vec<_> = (0..8)
            .map(|worker| {
                let engine = engine.clone();
                std::thread::spawn(move || {
                    for round in 0..25 {
                        engine
                            .write_cache_checked("shared.json", &vec![worker, round])
                            .unwrap_or_else(|e| panic!("worker {worker} round {round}: {e}"));
                    }
                })
            })
            .collect();
        for worker in workers {
            worker.join().unwrap();
        }
        let (_, data): (u64, Vec<u32>) = engine.read_cache_any("shared.json").expect("readable");
        assert_eq!(data.len(), 2);
        // No scratch files left behind.
        let leftovers = std::fs::read_dir(&dir)
            .unwrap()
            .filter(|e| {
                e.as_ref()
                    .unwrap()
                    .file_name()
                    .to_string_lossy()
                    .ends_with(".tmp")
            })
            .count();
        assert_eq!(leftovers, 0);
        std::fs::remove_dir_all(dir).unwrap();
    }
}
