//! Getting a line onto disk, and reading the tail of it back.
//!
//! Split out of `applog.rs` when levels, redaction and the panic hook moved in
//! there: this half is the part that touches the filesystem, and it is the
//! part with the fiddly arithmetic (the UTC timestamp) worth testing on its
//! own.
//!
//! Deliberately tiny and dependency-free. It opens the file per line rather
//! than holding a handle, because the volume is a handful of lines per session
//! and a handle would need locking and a flush-on-exit that a crash skips
//! anyway. Every failure here is handed back rather than raised: a logger that
//! panics because the disk is full turns a warning into a crash, which is
//! strictly worse than a lost log line.

use std::io::Write;
use std::path::{Path, PathBuf};
use std::time::{SystemTime, UNIX_EPOCH};

/// The log's name inside the app data directory.
pub const LOG_NAME: &str = "draft-assistant.log";

/// Rotate past a megabyte. One session writes a few hundred bytes, so this is
/// roughly "keep the last few thousand sessions" -- large enough to be useful,
/// small enough that nothing has to think about it.
const MAX_BYTES: u64 = 1024 * 1024;

/// Append one line, rotating first if the file has grown past the cap.
///
/// Takes a path rather than reading the process-wide directory so the tests
/// can drive it anywhere -- one test setting that `OnceLock` would wedge it
/// for every other test in the binary.
pub(super) fn append(path: &Path, line: &str) -> std::io::Result<()> {
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
pub(super) fn rotated(path: &Path) -> PathBuf {
    let mut name = path.as_os_str().to_os_string();
    name.push(".1");
    PathBuf::from(name)
}

/// The last `lines` lines of the log, oldest first.
///
/// Empty for a log that does not exist yet, which on a healthy first run is
/// the normal answer rather than a problem worth reporting. Only the current
/// generation is read: a line that has just rotated into `.1` is gone from
/// the dialog, which is the price of not loading two megabytes to show two
/// hundred lines.
pub fn tail(path: &Path, lines: usize) -> Vec<String> {
    let Ok(text) = std::fs::read_to_string(path) else {
        return Vec::new();
    };
    let all: Vec<&str> = text.lines().collect();
    all[all.len().saturating_sub(lines)..]
        .iter()
        .map(|line| (*line).to_string())
        .collect()
}

pub(super) fn now_secs() -> u64 {
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
pub(super) fn timestamp(secs: u64) -> String {
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

/// A scratch directory that cleans itself up. Never the real data directory:
/// these tests write megabytes, and a log write in a test process must not
/// touch anything a user would open.
#[cfg(test)]
pub(super) struct TempDir(PathBuf);

#[cfg(test)]
impl TempDir {
    pub(super) fn new(tag: &str) -> Self {
        let path = std::env::temp_dir().join(format!(
            "draft-assistant-applog-{tag}-{}-{}",
            std::process::id(),
            now_secs()
        ));
        std::fs::create_dir_all(&path).expect("create temp dir");
        Self(path)
    }

    pub(super) fn join(&self, name: &str) -> PathBuf {
        self.0.join(name)
    }
}

#[cfg(test)]
impl Drop for TempDir {
    fn drop(&mut self) {
        let _ = std::fs::remove_dir_all(&self.0);
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_first_line_creates_the_file_and_the_next_one_is_appended_below_it() {
        let dir = TempDir::new("append");
        let log = dir.join(LOG_NAME);
        append(&log, "one\n").expect("first write");
        append(&log, "two\n").expect("second write");
        assert_eq!(std::fs::read_to_string(&log).unwrap(), "one\ntwo\n");
    }

    #[test]
    fn a_missing_parent_directory_is_created_rather_than_failing_the_write() {
        let dir = TempDir::new("mkdir");
        let log = dir.join("nested").join("deeper").join(LOG_NAME);
        append(&log, "line\n").expect("write into a directory that did not exist");
        assert_eq!(std::fs::read_to_string(&log).unwrap(), "line\n");
    }

    #[test]
    fn a_log_past_the_cap_is_moved_aside_and_the_new_line_starts_a_fresh_file() {
        let dir = TempDir::new("rotate");
        let log = dir.join(LOG_NAME);
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
        let log = dir.join(LOG_NAME);
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
        let log = dir.join(LOG_NAME);
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
    fn the_tail_is_the_last_lines_in_the_order_they_were_written() {
        let dir = TempDir::new("tail");
        let log = dir.join(LOG_NAME);
        for n in 0..10 {
            append(&log, &format!("line {n}\n")).expect("write");
        }
        assert_eq!(tail(&log, 3), vec!["line 7", "line 8", "line 9"]);
        // Asking for more than there is gives everything, not a panic.
        assert_eq!(tail(&log, 100).len(), 10);
    }

    #[test]
    fn the_tail_of_a_log_that_does_not_exist_yet_is_empty_rather_than_an_error() {
        let dir = TempDir::new("tail-missing");
        assert!(tail(&dir.join(LOG_NAME), 10).is_empty());
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
