//! The on-disk cache envelope and the two operations every cached fetch needs.
//!
//! These are free functions over a path rather than methods on `Engine`, so
//! the same parse serves both the synchronous callers and the ones that push
//! the work onto the blocking pool. The players dictionary alone is ~15 MB of
//! JSON, which is far too much to parse on the async runtime.

use crate::engine::now_secs;
use serde::{Deserialize, Serialize};
use std::path::PathBuf;

/// What a cache file holds: the payload plus when it was fetched.
#[derive(Serialize, Deserialize)]
pub(crate) struct Cached<T> {
    pub fetched_at: u64,
    pub data: T,
}

/// Read and parse one cache envelope. A missing, unreadable or unparseable
/// file is simply a miss — the caller refetches.
pub(crate) fn read_cached<T: serde::de::DeserializeOwned>(path: PathBuf) -> Option<(u64, T)> {
    let raw = std::fs::read_to_string(path).ok()?;
    let cached: Cached<T> = serde_json::from_str(&raw).ok()?;
    Some((cached.fetched_at, cached.data))
}

/// Drop a cache hit that has aged past its TTL.
pub(crate) fn fresh_enough<T>(hit: Option<(u64, T)>, ttl: u64) -> Option<(u64, T)> {
    let (fetched_at, data) = hit?;
    (now_secs().saturating_sub(fetched_at) <= ttl).then_some((fetched_at, data))
}

/// Strip anything that could steer a path out of the cache directory. Sleeper
/// ids are alphanumeric, so this is lossless for real input and refuses `..`
/// and separators outright.
pub(crate) fn safe_key(raw: &str) -> String {
    raw.chars()
        .filter(|c| c.is_ascii_alphanumeric() || *c == '-' || *c == '_')
        .collect()
}

/// Wrap a payload in its envelope and serialize it.
pub(crate) fn envelope_json<T: Serialize>(fetched_at: u64, data: &T) -> Result<String, String> {
    serde_json::to_string(&Cached { fetched_at, data }).map_err(|e| format!("serialize: {e}"))
}

/// Write to a temp file, then rename over the target. The rename is atomic, so
/// a crash mid-write leaves the previous cache intact rather than a truncated
/// file that fails to parse.
pub(crate) fn replace_file(tmp: PathBuf, final_path: PathBuf, json: String) -> Result<(), String> {
    std::fs::write(&tmp, json).map_err(|e| format!("write {}: {e}", tmp.display()))?;
    std::fs::rename(&tmp, &final_path).map_err(|e| format!("replace {}: {e}", final_path.display()))
}

/// Serialize and atomically replace, in one step.
pub(crate) fn write_atomic<T: Serialize>(
    tmp: PathBuf,
    final_path: PathBuf,
    fetched_at: u64,
    data: &T,
) -> Result<(), String> {
    replace_file(tmp, final_path, envelope_json(fetched_at, data)?)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn temp(label: &str) -> PathBuf {
        let dir = std::env::temp_dir().join(format!(
            "draft-assistant-cache-{label}-{}-{}",
            std::process::id(),
            now_secs()
        ));
        std::fs::create_dir_all(&dir).unwrap();
        dir
    }

    #[test]
    fn a_written_envelope_reads_back_with_its_timestamp() {
        let dir = temp("roundtrip");
        let target = dir.join("thing.json");
        write_atomic(
            dir.join("thing.tmp"),
            target.clone(),
            1234,
            &vec![1u32, 2, 3],
        )
        .unwrap();
        let (at, data): (u64, Vec<u32>) = read_cached(target).unwrap();
        assert_eq!(at, 1234);
        assert_eq!(data, vec![1, 2, 3]);
        // The temp file is renamed away, never left behind.
        assert!(!dir.join("thing.tmp").exists());
        std::fs::remove_dir_all(dir).unwrap();
    }

    #[test]
    fn a_missing_or_corrupt_file_is_a_miss_rather_than_an_error() {
        let dir = temp("corrupt");
        assert!(read_cached::<Vec<u32>>(dir.join("absent.json")).is_none());
        let broken = dir.join("broken.json");
        std::fs::write(&broken, "{not json").unwrap();
        assert!(read_cached::<Vec<u32>>(broken).is_none());
        std::fs::remove_dir_all(dir).unwrap();
    }

    #[test]
    fn safe_key_refuses_anything_that_could_leave_the_cache_directory() {
        assert_eq!(safe_key("1389710366300200960"), "1389710366300200960");
        assert_eq!(safe_key("draft-1_a"), "draft-1_a");
        assert_eq!(safe_key("../../etc/passwd"), "etcpasswd");
        assert_eq!(safe_key("a/b\\c"), "abc");
        assert_eq!(safe_key(""), "");
    }

    #[test]
    fn a_hit_past_its_ttl_is_dropped() {
        let now = now_secs();
        assert!(fresh_enough(Some((now, 1)), 60).is_some());
        assert!(fresh_enough(Some((now - 30, 1)), 60).is_some());
        assert!(fresh_enough(Some((now - 600, 1)), 60).is_none());
        assert!(fresh_enough::<u32>(None, 60).is_none());
    }
}
