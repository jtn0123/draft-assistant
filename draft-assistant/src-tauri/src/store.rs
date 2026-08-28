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
use std::path::PathBuf;

impl Engine {
    pub(crate) fn cache_path(&self, name: &str) -> PathBuf {
        self.data_dir.join(name)
    }

    pub(crate) fn read_cache<T: serde::de::DeserializeOwned>(
        &self,
        name: &str,
        ttl: u64,
    ) -> Option<(u64, T)> {
        let (fetched_at, data) = self.read_cache_any(name)?;
        if now_secs().saturating_sub(fetched_at) > ttl {
            return None;
        }
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
        let tmp = self.cache_path(&format!("{name}.tmp"));
        std::fs::write(&tmp, json).map_err(|e| format!("write {}: {e}", tmp.display()))?;
        std::fs::rename(&tmp, self.cache_path(name)).map_err(|e| format!("replace {name}: {e}"))?;
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
        let tmp = self.cache_path("config.json.tmp");
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
}
