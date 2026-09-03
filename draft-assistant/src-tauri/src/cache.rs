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
///
/// A timestamp in the future is a miss too. It means the clock was wrong when
/// the file was written, or has since been set back, and `saturating_sub`
/// floors the age of such a file at zero — so without this it would read as
/// freshly fetched for as long as it sat on disk, and no refresh would ever
/// happen.
pub(crate) fn fresh_enough<T>(hit: Option<(u64, T)>, ttl: u64) -> Option<(u64, T)> {
    let (fetched_at, data) = hit?;
    let now = now_secs();
    if fetched_at > now {
        return None;
    }
    (now - fetched_at <= ttl).then_some((fetched_at, data))
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

/// Lock a freshly written cache file down to its owner.
///
/// Cache files hold league rosters, member names and Sleeper user ids. The
/// default 0644 leaves all of that readable by every account and every process
/// on the machine, so permissions are narrowed before the file is put in
/// place. Unix only; Windows has no equivalent mode and keeps the default.
pub(crate) fn owner_only(path: &std::path::Path) {
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        std::fs::set_permissions(path, std::fs::Permissions::from_mode(0o600)).ok();
    }
    #[cfg(not(unix))]
    let _ = path;
}

/// Lock a directory we created down to its owner, for the same reason.
pub(crate) fn owner_only_dir(path: &std::path::Path) {
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        std::fs::set_permissions(path, std::fs::Permissions::from_mode(0o700)).ok();
    }
    #[cfg(not(unix))]
    let _ = path;
}

/// Write to a temp file, then rename over the target. The rename is atomic, so
/// a crash mid-write leaves the previous cache intact rather than a truncated
/// file that fails to parse.
///
/// The mode is narrowed before the rename, so the file is never visible to
/// anyone else even for an instant.
pub(crate) fn replace_file(tmp: PathBuf, final_path: PathBuf, json: String) -> Result<(), String> {
    std::fs::write(&tmp, json).map_err(|e| format!("write {}: {e}", tmp.display()))?;
    owner_only(&tmp);
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

    /// Cache files hold rosters, league member names and Sleeper user ids.
    /// Written at the default 0644 they were readable by every other account
    /// and process on the machine.
    #[cfg(unix)]
    #[test]
    fn a_written_cache_file_is_readable_only_by_its_owner() {
        use std::os::unix::fs::PermissionsExt;
        let dir = temp("mode");
        let target = dir.join("private.json");
        write_atomic(dir.join("private.tmp"), target.clone(), 1, &vec![1u32]).unwrap();
        let mode = std::fs::metadata(&target).unwrap().permissions().mode();
        assert_eq!(
            mode & 0o777,
            0o600,
            "cache file mode was {:o}",
            mode & 0o777
        );
        std::fs::remove_dir_all(dir).unwrap();
    }

    #[cfg(unix)]
    #[test]
    fn a_cache_directory_is_reachable_only_by_its_owner() {
        use std::os::unix::fs::PermissionsExt;
        let dir = temp("dirmode");
        owner_only_dir(&dir);
        let mode = std::fs::metadata(&dir).unwrap().permissions().mode();
        assert_eq!(mode & 0o777, 0o700, "cache dir mode was {:o}", mode & 0o777);
        std::fs::remove_dir_all(dir).unwrap();
    }

    #[test]
    fn a_hit_past_its_ttl_is_dropped() {
        let now = now_secs();
        assert!(fresh_enough(Some((now, 1)), 60).is_some());
        assert!(fresh_enough(Some((now - 30, 1)), 60).is_some());
        assert!(fresh_enough(Some((now - 600, 1)), 60).is_none());
        assert!(fresh_enough::<u32>(None, 60).is_none());
    }

    /// A file stamped in the future never ages: its age floors at zero, so it
    /// stayed "fresh" forever and the payload behind it was never refetched.
    #[test]
    fn a_hit_stamped_in_the_future_is_a_miss_rather_than_fresh_forever() {
        let now = now_secs();
        assert!(fresh_enough(Some((now + 1, 1)), 60).is_none());
        assert!(fresh_enough(Some((now + 86_400, 1)), 60).is_none());
        // The boundary itself is still a hit: `now` is not in the future.
        assert!(fresh_enough(Some((now, 1)), 60).is_some());
    }
}
