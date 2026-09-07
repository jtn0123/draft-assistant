//! The cache writes that failed, kept for the user and for the log.
//!
//! Split out of `engine.rs`, which is at the file cap.

use crate::engine::Engine;
use std::collections::HashSet;

/// The cache writes that have failed: the ones not yet shown to the user,
/// and the names of every file whose failure has already been logged.
#[derive(Default)]
pub(crate) struct CacheFailures {
    pending: Vec<(String, String)>,
    logged: HashSet<String>,
}

impl Engine {
    /// Remember that a cache write failed, at most once per cache file, and
    /// put the first failure of each file in the log.
    ///
    /// Deduplicated by name rather than by message: the detail carries the
    /// temp file's own unique name, so a poll tick failing to write the same
    /// key every three seconds would otherwise stack up one warning per tick.
    /// The log line is once per file for the life of the engine, however
    /// many loads drain the pending list in between: the failure is the same
    /// full disk or read-only directory every time, and a log that repeats it
    /// per load buries whatever else went wrong.
    pub(crate) fn note_cache_failure(&self, name: &str, detail: &str) {
        let Ok(mut failures) = self.cache_warnings.lock() else {
            return;
        };
        let message = format!("{name} was not cached: {detail}");
        if failures.logged.insert(name.to_string()) {
            crate::applog::warn(&message);
        }
        if failures.pending.iter().all(|(seen, _)| seen != name) {
            failures.pending.push((name.to_string(), message));
        }
    }

    /// Take the cache-write failures collected since the last load, so one
    /// load reports each of them once.
    pub(crate) fn take_cache_warnings(&self) -> Vec<String> {
        self.cache_warnings
            .lock()
            .map(|mut failures| {
                std::mem::take(&mut failures.pending)
                    .into_iter()
                    .map(|(_, m)| m)
                    .collect()
            })
            .unwrap_or_default()
    }
}
