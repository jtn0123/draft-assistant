//! Season cache writes that say so when they fail.
//!
//! `Engine::write_cache_off_thread` remembers a failed write for the next
//! league load to put on screen. That is the right shape for the draft
//! caches, which are written by a load. The season files, the per-week
//! matchups, the rosters, the NFL state, the Trends history, are written by
//! the poller and by the rollover, hours after any load: a disk that filled
//! up on Sunday afternoon was remembered and shown to nobody, and nothing in
//! the log said the caches had stopped landing.

use crate::cache::{envelope_json_off_runtime, replace_file, temp_sibling};
use crate::engine::{now_secs, Engine};
use serde::Serialize;

impl Engine {
    /// Write one season cache file off the runtime. A failure is logged at
    /// warn the moment it happens, and still remembered for the next load.
    pub(crate) async fn write_season_cache<T: Serialize>(&self, name: &str, data: &T) -> u64 {
        let fetched_at = now_secs();
        let json = match envelope_json_off_runtime(fetched_at, data) {
            Ok(json) => json,
            Err(error) => {
                self.season_cache_failed(name, &error);
                return fetched_at;
            }
        };
        let final_path = self.data_dir.join(name);
        let tmp = temp_sibling(&final_path);
        let written =
            tokio::task::spawn_blocking(move || replace_file(tmp, final_path, json)).await;
        match written {
            Ok(Ok(())) => {}
            Ok(Err(error)) => self.season_cache_failed(name, &error),
            Err(error) => self.season_cache_failed(name, &error.to_string()),
        }
        fetched_at
    }

    fn season_cache_failed(&self, name: &str, detail: &str) {
        crate::applog::warn(format!("season cache {name} was not written: {detail}"));
        self.note_cache_failure(name, detail);
    }
}

#[cfg(test)]
mod tests {
    use crate::engine::Engine;

    fn scratch(label: &str) -> std::path::PathBuf {
        std::env::temp_dir().join(format!(
            "draft-assistant-season-cache-{label}-{}-{}",
            std::process::id(),
            crate::engine::now_secs()
        ))
    }

    #[tokio::test]
    async fn a_good_write_lands_as_a_readable_envelope() {
        let dir = scratch("ok");
        let engine = Engine::new(dir.clone());
        let capture = crate::applog::Capture::start();
        engine
            .write_season_cache("season_x_week1.json", &vec![1, 2])
            .await;
        let (_, rows) = engine
            .read_cache_any::<Vec<u32>>("season_x_week1.json")
            .expect("the file was written");
        assert_eq!(rows, vec![1, 2]);
        assert!(capture.lines().is_empty(), "{:?}", capture.lines());
        std::fs::remove_dir_all(dir).ok();
    }

    /// The bug: a season cache that failed to write was remembered for a load
    /// that might never come and never logged, so a full disk on a Sunday
    /// left no trace anywhere.
    #[tokio::test]
    async fn a_failed_write_is_logged_at_warn_and_still_remembered() {
        // A data directory that is a file: nothing can be created inside it.
        let dir = scratch("blocked");
        std::fs::create_dir_all(dir.parent().unwrap()).unwrap();
        std::fs::write(&dir, b"not a directory").unwrap();
        let engine = Engine::new(dir.clone());
        let capture = crate::applog::Capture::start();

        engine
            .write_season_cache("season_x_week1.json", &vec![1])
            .await;

        assert!(
            capture.saw("WARN season cache season_x_week1.json was not written"),
            "{:?}",
            capture.lines()
        );
        let remembered = engine.take_cache_warnings();
        assert!(
            remembered
                .iter()
                .any(|w| w.starts_with("season_x_week1.json was not cached")),
            "{remembered:?}"
        );
        std::fs::remove_file(dir).ok();
    }
}
