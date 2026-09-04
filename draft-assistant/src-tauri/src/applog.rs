//! An append-only warning log, so a problem the user hit yesterday is still
//! readable today.
//!
//! Warnings used to go to stderr only, which in a bundled `.app` means they go
//! nowhere at all: double-clicking Draft Assistant gives it no terminal, so
//! every "projection source unreachable" or "cache write failed" was written
//! straight into the void. This writes them to `draft-assistant.log` beside
//! the app's other data instead.
//!
//! Deliberately tiny and dependency-free. It opens the file per line rather
//! than holding a handle, because the volume is a handful of lines per session
//! and a handle would need locking and a flush-on-exit that a crash skips
//! anyway. Every failure here is swallowed: a logger that panics because the
//! disk is full turns a warning into a crash, which is strictly worse than a
//! lost log line.

use std::io::Write;
use std::path::{Path, PathBuf};
use std::sync::OnceLock;
use std::time::{SystemTime, UNIX_EPOCH};

/// Where the log lives, once the app knows its data directory.
///
/// Set once from `lib.rs`'s `setup`. Anything that warns before that -- and
/// every unit test, every `cargo test`, both `dump_*` binaries -- falls back to
/// stderr, so nothing has to care whether the app is running.
static DIR: OnceLock<PathBuf> = OnceLock::new();

/// Rotate past a megabyte. One session writes a few hundred bytes, so this is
/// roughly "keep the last few thousand sessions" -- large enough to be useful,
/// small enough that nothing has to think about it.
const MAX_BYTES: u64 = 1024 * 1024;

/// Point the log at the app data directory. Later calls are ignored: the
/// directory does not change while the app runs, and a second call is a bug
/// worth ignoring rather than panicking over.
pub fn init(dir: PathBuf) {
    let _ = DIR.set(dir);
}

/// Record one warning. Timestamped, one line, never fails loudly.
pub fn warn(msg: impl AsRef<str>) {
    let msg = msg.as_ref();
    let line = format!("{} WARN {msg}\n", timestamp(now_secs()));
    match DIR.get() {
        Some(dir) => {
            if append(&dir.join("draft-assistant.log"), &line).is_err() {
                // The log is the thing that broke, so stderr is all that is
                // left. Silently dropping it would hide the original warning.
                eprint!("{line}");
            }
        }
        None => eprint!("{line}"),
    }
}

/// Append one line, rotating first if the file has grown past the cap.
///
/// Split out from `warn` so the tests can drive it at a temp path -- `warn`
/// itself reads the process-wide `OnceLock`, which one test setting would
/// wedge for every other test in the binary.
fn append(path: &Path, line: &str) -> std::io::Result<()> {
    if let Some(parent) = path.parent() {
        std::fs::create_dir_all(parent)?;
    }
    rotate_if_full(path)?;
    let mut file = std::fs::OpenOptions::new()
        .create(true)
        .append(true)
        .open(path)?;
    file.write_all(line.as_bytes())
}

/// One generation of history: the full log becomes `.1`, replacing whatever
/// `.1` held. Two files bound the disk cost at 2 MB, and the older one is
/// almost never the interesting one.
fn rotate_if_full(path: &Path) -> std::io::Result<()> {
    let too_big = match std::fs::metadata(path) {
        Ok(meta) => meta.len() > MAX_BYTES,
        // No file yet is the normal first-run case, not an error.
        Err(_) => false,
    };
    if too_big {
        std::fs::rename(path, rotated(path))?;
    }
    Ok(())
}

/// `draft-assistant.log` -> `draft-assistant.log.1`.
fn rotated(path: &Path) -> PathBuf {
    let mut name = path.as_os_str().to_os_string();
    name.push(".1");
    PathBuf::from(name)
}

fn now_secs() -> u64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|d| d.as_secs())
        // A clock set before 1970 is not worth a branch anywhere else.
        .unwrap_or(0)
}

/// `2026-09-03T14:22:01Z` from a Unix second count, in UTC.
///
/// Hand-rolled because the alternative is pulling `chrono` in for one line of
/// output. The civil-date arithmetic is Howard Hinnant's `civil_from_days`,
/// which is exact for every date this app will ever see.
fn timestamp(secs: u64) -> String {
    let days = (secs / 86_400) as i64;
    let time_of_day = secs % 86_400;
    let (year, month, day) = civil_from_days(days);
    let (hour, minute, second) = (
        time_of_day / 3_600,
        (time_of_day % 3_600) / 60,
        time_of_day % 60,
    );
    format!("{year:04}-{month:02}-{day:02}T{hour:02}:{minute:02}:{second:02}Z")
}

/// Days since 1970-01-01 to a civil (year, month, day), UTC.
///
/// Shifts the epoch to 0000-03-01 so that the leap day lands at the end of the
/// year and the month lengths form a repeating pattern with no special case
/// for February.
fn civil_from_days(days: i64) -> (i64, u32, u32) {
    let z = days + 719_468;
    let era = if z >= 0 { z } else { z - 146_096 } / 146_097;
    let day_of_era = (z - era * 146_097) as u64; // [0, 146096]
    let year_of_era =
        (day_of_era - day_of_era / 1460 + day_of_era / 36_524 - day_of_era / 146_096) / 365;
    let year = year_of_era as i64 + era * 400;
    let day_of_year = day_of_era - (365 * year_of_era + year_of_era / 4 - year_of_era / 100);
    let mp = (5 * day_of_year + 2) / 153; // [0, 11], March-based
    let day = (day_of_year - (153 * mp + 2) / 5 + 1) as u32;
    let month = if mp < 10 { mp + 3 } else { mp - 9 } as u32;
    (if month <= 2 { year + 1 } else { year }, month, day)
}

#[cfg(test)]
mod tests {
    use super::*;

    /// A scratch directory that cleans itself up. Never the real data
    /// directory: these tests write megabytes, and `warn` in a test process
    /// must not touch anything a user would open.
    struct TempDir(PathBuf);

    impl TempDir {
        fn new(tag: &str) -> Self {
            let path = std::env::temp_dir().join(format!(
                "draft-assistant-applog-{tag}-{}-{}",
                std::process::id(),
                now_secs()
            ));
            std::fs::create_dir_all(&path).expect("create temp dir");
            Self(path)
        }

        fn join(&self, name: &str) -> PathBuf {
            self.0.join(name)
        }
    }

    impl Drop for TempDir {
        fn drop(&mut self) {
            let _ = std::fs::remove_dir_all(&self.0);
        }
    }

    #[test]
    fn the_first_line_creates_the_file_and_the_next_one_is_appended_below_it() {
        let dir = TempDir::new("append");
        let log = dir.join("draft-assistant.log");
        append(&log, "one\n").expect("first write");
        append(&log, "two\n").expect("second write");
        assert_eq!(std::fs::read_to_string(&log).unwrap(), "one\ntwo\n");
    }

    #[test]
    fn a_missing_parent_directory_is_created_rather_than_failing_the_write() {
        let dir = TempDir::new("mkdir");
        let log = dir
            .join("nested")
            .join("deeper")
            .join("draft-assistant.log");
        append(&log, "line\n").expect("write into a directory that did not exist");
        assert_eq!(std::fs::read_to_string(&log).unwrap(), "line\n");
    }

    #[test]
    fn a_log_past_the_cap_is_moved_aside_and_the_new_line_starts_a_fresh_file() {
        let dir = TempDir::new("rotate");
        let log = dir.join("draft-assistant.log");
        let old = "x".repeat(MAX_BYTES as usize + 1);
        std::fs::write(&log, &old).expect("seed an oversized log");

        append(&log, "after\n").expect("write past the cap");

        // The new file holds only the new line...
        assert_eq!(std::fs::read_to_string(&log).unwrap(), "after\n");
        // ...and the old contents survived in .1 rather than being dropped.
        assert_eq!(std::fs::read_to_string(rotated(&log)).unwrap(), old);
    }

    #[test]
    fn a_log_under_the_cap_is_left_alone() {
        let dir = TempDir::new("norotate");
        let log = dir.join("draft-assistant.log");
        std::fs::write(&log, "small\n").expect("seed");
        append(&log, "more\n").expect("write");
        assert_eq!(std::fs::read_to_string(&log).unwrap(), "small\nmore\n");
        assert!(
            !rotated(&log).exists(),
            "nothing should have been rotated aside"
        );
    }

    #[test]
    fn rotating_twice_overwrites_the_previous_backup_rather_than_growing_forever() {
        let dir = TempDir::new("rotate-twice");
        let log = dir.join("draft-assistant.log");
        let over_cap = "x".repeat(MAX_BYTES as usize + 1);

        std::fs::write(&log, "first generation").expect("seed .1's eventual contents");
        std::fs::rename(&log, rotated(&log)).expect("pre-rotate");
        std::fs::write(&log, &over_cap).expect("seed an oversized log");
        append(&log, "after\n").expect("write past the cap");

        assert_eq!(std::fs::read_to_string(rotated(&log)).unwrap(), over_cap);
        // Two files, never three: no .2 is left behind.
        let mut name = rotated(&log).as_os_str().to_os_string();
        name.push(".1");
        assert!(!PathBuf::from(name).exists(), "history is one generation");
    }

    #[test]
    fn warn_before_init_writes_nothing_to_disk_and_does_not_panic() {
        // No `init` has run in this test binary, so `DIR` is empty and the
        // fallback is stderr. The assertion that matters is that this neither
        // panics nor creates a file anywhere the test can see.
        let dir = TempDir::new("preinit");
        warn("engine could not reach the projection source");
        assert!(
            !dir.join("draft-assistant.log").exists(),
            "a pre-init warn must not invent a log file"
        );
        assert!(DIR.get().is_none(), "no test in this binary may call init");
    }

    #[test]
    fn every_line_carries_a_sortable_utc_timestamp() {
        // Every expected string below came from an independent implementation
        // (Python's `datetime.fromtimestamp(..., timezone.utc)`), not from
        // running this function and writing down what it said.
        assert_eq!(timestamp(1_788_452_521), "2026-09-03T16:22:01Z");
        assert_eq!(timestamp(0), "1970-01-01T00:00:00Z");
        // A leap day, the case the March-based shift exists to get right.
        assert_eq!(timestamp(1_709_209_845), "2024-02-29T12:30:45Z");
        // 2100 is not a leap year, which is the rule a naive "every four
        // years" conversion gets wrong and this one has to get right.
        assert_eq!(timestamp(4_102_444_800), "2100-01-01T00:00:00Z");
        // Lexical order matches chronological order, which is the only
        // property the log actually depends on.
        assert!(timestamp(1_000_000) < timestamp(2_000_000));
    }
}
