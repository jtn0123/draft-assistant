//! The on-disk cache envelope and the two operations every cached fetch needs.
//!
//! These are free functions over a path rather than methods on `Engine`, so
//! the same parse serves both the synchronous callers and the ones that push
//! the work onto the blocking pool. The players dictionary alone is ~15 MB of
//! JSON, which is far too much to parse on the async runtime.

use crate::engine::now_secs;
use serde::{Deserialize, Serialize};
use std::io::Write;
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicU64, Ordering};

/// How long a leftover temp file has to sit before a startup sweep collects
/// it. Long enough that a write in flight in another copy of the app — or in
/// another test in the same binary — is never pulled out from under it.
pub(crate) const TEMP_STALE_SECS: u64 = 3600;

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

/// A temp file name nobody else will pick.
///
/// The old name was `{key}.tmp`, one per cache key: two writers of the same
/// key — a poll tick and a manual refresh, or two windows of the app — wrote
/// into the very same file at the same time and then each renamed the
/// interleaved result over a cache that had been fine. The pid and a counter
/// make the name unique per writer, so concurrent writes cannot mix and the
/// rename that lands last simply wins with a whole file.
pub(crate) fn temp_sibling(final_path: &Path) -> PathBuf {
    static NEXT: AtomicU64 = AtomicU64::new(0);
    let nonce = NEXT.fetch_add(1, Ordering::Relaxed);
    let name = final_path
        .file_name()
        .map(|n| n.to_string_lossy().to_string())
        .unwrap_or_else(|| "cache".to_string());
    let unique = format!("{name}.{}.{nonce}.tmp", std::process::id());
    final_path
        .parent()
        .map(|dir| dir.join(&unique))
        .unwrap_or_else(|| PathBuf::from(unique))
}

/// Remove temp files an earlier run left behind.
///
/// A write that is interrupted between the temp file and the rename leaves
/// its temp file forever, and with unique names those accumulate one per
/// crash rather than being overwritten. Anything older than
/// [`TEMP_STALE_SECS`] cannot belong to a write still in progress.
pub(crate) fn sweep_stale_temp_files(dir: &Path) {
    let Ok(entries) = std::fs::read_dir(dir) else {
        return;
    };
    for entry in entries.flatten() {
        let path = entry.path();
        if path.extension().is_none_or(|ext| ext != "tmp") {
            continue;
        }
        if temp_file_is_stale(&path, TEMP_STALE_SECS) {
            std::fs::remove_file(&path).ok();
        }
    }
}

/// Whether one temp file is old enough to collect. A file whose mtime cannot
/// be read, or is in the future, is left alone: guessing wrong here deletes
/// somebody's write in flight.
fn temp_file_is_stale(path: &Path, older_than: u64) -> bool {
    let Ok(modified) = std::fs::metadata(path).and_then(|m| m.modified()) else {
        return false;
    };
    let Ok(age) = modified.elapsed() else {
        return false;
    };
    age.as_secs() > older_than
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
    write_synced(&tmp, json.as_bytes()).map_err(|e| format!("write {}: {e}", tmp.display()))?;
    owner_only(&tmp);
    std::fs::rename(&tmp, &final_path).map_err(|e| format!("replace {}: {e}", final_path.display()))
}

/// Write a file and make sure the bytes are actually on the disk before the
/// caller renames it into place.
///
/// Without the `sync_all` the rename can reach the disk before the contents
/// do: a power cut or a hard reset in that window leaves the cache entry
/// pointing at a file of zeros, which reads back as a parse failure rather
/// than as the previous good copy the atomic rename was supposed to protect.
pub(crate) fn write_synced(path: &Path, bytes: &[u8]) -> std::io::Result<()> {
    let mut file = std::fs::File::create(path)?;
    file.write_all(bytes)?;
    file.sync_all()
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
        let tmp = temp_sibling(&target);
        write_atomic(tmp.clone(), target.clone(), 1234, &vec![1u32, 2, 3]).unwrap();
        let (at, data): (u64, Vec<u32>) = read_cached(target).unwrap();
        assert_eq!(at, 1234);
        assert_eq!(data, vec![1, 2, 3]);
        // The temp file is renamed away, never left behind.
        assert!(!tmp.exists());
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

    /// Two writers of the same cache key used to share one `{key}.tmp`: they
    /// interleaved into it and each renamed the mixture over a cache file
    /// that had been perfectly good. With a name per writer, whichever
    /// rename lands last leaves a whole, parseable file.
    #[test]
    fn two_concurrent_writers_of_one_key_leave_a_valid_file() {
        let dir = temp("concurrent");
        let target = dir.join("shared.json");
        let payload_a = vec![1u32; 4000];
        let payload_b = vec![2u32; 4000];
        let handles: Vec<_> = [payload_a.clone(), payload_b.clone()]
            .into_iter()
            .map(|payload| {
                let target = target.clone();
                std::thread::spawn(move || {
                    for _ in 0..20 {
                        write_atomic(temp_sibling(&target), target.clone(), 7, &payload).unwrap();
                    }
                })
            })
            .collect();
        for handle in handles {
            handle.join().unwrap();
        }
        let (at, data): (u64, Vec<u32>) = read_cached(target).expect("a parseable cache file");
        assert_eq!(at, 7);
        assert!(data == payload_a || data == payload_b, "torn payload");
        // Every temp file was renamed away.
        let leftovers: Vec<_> = std::fs::read_dir(&dir)
            .unwrap()
            .flatten()
            .map(|e| e.path())
            .filter(|p| p.extension().is_some_and(|x| x == "tmp"))
            .collect();
        assert!(leftovers.is_empty(), "left behind {leftovers:?}");
        std::fs::remove_dir_all(dir).unwrap();
    }

    #[test]
    fn temp_names_are_unique_per_writer() {
        let target = PathBuf::from("/somewhere/players.json");
        let first = temp_sibling(&target);
        let second = temp_sibling(&target);
        assert_ne!(first, second);
        assert_eq!(first.parent(), Some(std::path::Path::new("/somewhere")));
        assert_eq!(first.extension().unwrap(), "tmp");
    }

    /// A write killed between its temp file and its rename leaves the temp
    /// file behind forever. The sweep collects the old ones and keeps its
    /// hands off anything recent, which may be a write still running.
    #[test]
    fn the_sweep_collects_old_temp_files_and_spares_fresh_ones() {
        let dir = temp("sweep");
        let stale = dir.join("players.json.1.0.tmp");
        let fresh = dir.join("players.json.1.1.tmp");
        let keeper = dir.join("players.json");
        for path in [&stale, &fresh, &keeper] {
            std::fs::write(path, "{}").unwrap();
        }
        let old =
            std::time::SystemTime::now() - std::time::Duration::from_secs(TEMP_STALE_SECS * 2);
        std::fs::File::options()
            .write(true)
            .open(&stale)
            .unwrap()
            .set_modified(old)
            .unwrap();

        sweep_stale_temp_files(&dir);
        assert!(!stale.exists(), "the old temp file should have been swept");
        assert!(fresh.exists(), "a fresh temp file may be a write in flight");
        assert!(keeper.exists(), "the cache itself is not a temp file");
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
        write_atomic(temp_sibling(&target), target.clone(), 1, &vec![1u32]).unwrap();
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
