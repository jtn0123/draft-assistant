//! Getting a line onto disk, and reading the tail of it back.
//!
//! Split out of `applog.rs` when levels, redaction and the panic hook moved in
//! there: this half is the part that touches the filesystem, and it is the
//! part with the fiddly arithmetic (the UTC timestamp) worth testing on its
//! own.
//!
//! Deliberately tiny and dependency-free. One handle is kept open between
//! lines: opening per line was fine at a few lines per session, but at the
//! debug level a poll loop writes several per tick, and an open is a path walk
//! on the runtime thread every time. Writes are unbuffered, so a crash loses
//! nothing that was handed over. Every failure here is handed back rather
//! than raised: a logger that panics because the disk is full turns a warning
//! into a crash, which is strictly worse than a lost log line.

use std::fs::File;
use std::io::Write;
use std::path::{Path, PathBuf};
use std::sync::Mutex;
use std::time::{SystemTime, UNIX_EPOCH};

/// The log's name inside the app data directory.
pub const LOG_NAME: &str = "draft-assistant.log";

/// Rotate past a megabyte. One session writes a few hundred bytes, so this is
/// roughly "keep the last few thousand sessions" -- large enough to be useful,
/// small enough that nothing has to think about it.
const MAX_BYTES: u64 = 1024 * 1024;

/// The handle the last line went through, kept so the next line does not pay
/// for an open and a close of its own.
///
/// Checked before every write with one `fstat`: past the cap it is let go so
/// the file can rotate, and gone from the directory it is let go so the file
/// comes back, rather than the process writing on into an unlinked inode
/// nobody can read.
static OPEN: Mutex<Option<OpenLog>> = Mutex::new(None);

struct OpenLog {
    path: PathBuf,
    file: File,
}

/// Append one line, rotating first if the file has grown past the cap.
///
/// Takes a path rather than reading the process-wide directory so the tests
/// can drive it anywhere -- one test setting that `OnceLock` would wedge it
/// for every other test in the binary.
pub(super) fn append(path: &Path, line: &str) -> std::io::Result<()> {
    let mut slot = OPEN.lock().unwrap_or_else(|e| e.into_inner());
    if !reusable(slot.as_ref(), path) {
        // Closed before the rename, so a rotation on a platform that will not
        // move an open file still goes through.
        *slot = None;
        rotate_if_full(path)?;
        *slot = Some(OpenLog {
            path: path.to_path_buf(),
            file: open_private(path)?,
        });
    }
    match slot.as_mut() {
        Some(open) => open.file.write_all(line.as_bytes()),
        None => Err(std::io::Error::other("the log handle was not opened")),
    }
}

/// Whether the kept handle is still the right one for `path`: the same file,
/// still in its directory, and still under the cap.
fn reusable(open: Option<&OpenLog>, path: &Path) -> bool {
    open.is_some_and(|open| {
        open.path == path
            && open
                .file
                .metadata()
                .is_ok_and(|meta| still_linked(&meta) && meta.len() <= MAX_BYTES)
    })
}

/// Whether the file behind a handle still has a name. A user who deletes the
/// log while the app runs expects a new one, not silence.
#[cfg(unix)]
fn still_linked(meta: &std::fs::Metadata) -> bool {
    use std::os::unix::fs::MetadataExt;
    meta.nlink() > 0
}

#[cfg(not(unix))]
fn still_linked(_meta: &std::fs::Metadata) -> bool {
    true
}

/// Open for append, creating the file readable by its owner alone.
///
/// The log quotes league names, device names and every URL that failed, and
/// it was the one file under the data directory created 0644: readable by
/// any other account on the machine while everything beside it was 0600. A
/// log left 0644 by an older build is narrowed on open rather than kept.
fn open_private(path: &Path) -> std::io::Result<File> {
    if let Some(parent) = path.parent() {
        std::fs::create_dir_all(parent)?;
    }
    let mut options = std::fs::OpenOptions::new();
    options.create(true).append(true);
    #[cfg(unix)]
    {
        use std::os::unix::fs::OpenOptionsExt;
        options.mode(0o600);
    }
    let file = options.open(path)?;
    // Best effort: a filesystem that refuses the chmod still gets the line,
    // because losing the log over its permissions helps nobody.
    let _ = narrow_to_owner(&file);
    Ok(file)
}

#[cfg(unix)]
fn narrow_to_owner(file: &File) -> std::io::Result<()> {
    use std::os::unix::fs::PermissionsExt;
    let mut perms = file.metadata()?.permissions();
    if perms.mode() & 0o077 != 0 {
        perms.set_mode(0o600);
        file.set_permissions(perms)?;
    }
    Ok(())
}

#[cfg(not(unix))]
fn narrow_to_owner(_file: &File) -> std::io::Result<()> {
    Ok(())
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

/// The last `lines` lines of the log, oldest first, across both generations.
///
/// Empty for a log that does not exist yet, which on a healthy first run is
/// the normal answer rather than a problem worth reporting.
///
/// `.1` is read when the current file is shorter than the ask. The failure
/// that prevents is the nastiest one this file has: a rotation happening a
/// moment before the thing went wrong leaves the current file two lines long,
/// and a tail that read only that showed two lines and hid the whole evening
/// that explains them.
pub fn tail(path: &Path, lines: usize) -> Vec<String> {
    let mut out = last_lines(path, lines);
    if out.len() < lines {
        // Only the tail end of the older generation is wanted, and it goes
        // above the newer lines because it was written first.
        let mut older = last_lines(&rotated(path), lines - out.len());
        older.append(&mut out);
        out = older;
    }
    out
}

/// The last `lines` lines of one file, or nothing when it is not there.
fn last_lines(path: &Path, lines: usize) -> Vec<String> {
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

    /// The handle is kept between lines, and the checks that decide when it
    /// cannot be: the file grew past the cap, was deleted, or is another file.
    #[test]
    fn the_kept_handle_is_reused_until_the_file_rotates_or_disappears() {
        let dir = TempDir::new("reuse");
        let log = dir.join(LOG_NAME);
        let open = OpenLog {
            path: log.clone(),
            file: open_private(&log).expect("open"),
        };
        assert!(reusable(Some(&open), &log), "a fresh handle is reused");
        assert!(
            !reusable(Some(&open), &dir.join("other.log")),
            "a handle on one file is not used for another"
        );
        assert!(!reusable(None, &log), "nothing open means open");

        std::fs::write(&log, "x".repeat(MAX_BYTES as usize + 1)).expect("grow past the cap");
        assert!(
            !reusable(Some(&open), &log),
            "past the cap the handle is let go so the file can rotate"
        );

        std::fs::write(&log, "small").expect("shrink");
        assert!(reusable(Some(&open), &log));
        std::fs::remove_file(&log).expect("delete the log");
        assert!(
            !reusable(Some(&open), &log),
            "a deleted log is let go so the next line recreates it"
        );
    }

    /// The failure this prevents: a log deleted mid-session went on being
    /// written into an unlinked inode, and the Diagnostics dialog showed an
    /// empty tail for the rest of the evening.
    #[test]
    fn a_log_deleted_while_the_app_runs_is_recreated_rather_than_written_into_the_void() {
        let dir = TempDir::new("deleted");
        let log = dir.join(LOG_NAME);
        append(&log, "before\n").expect("first write");
        std::fs::remove_file(&log).expect("delete it out from under the handle");
        append(&log, "after\n").expect("second write");
        assert_eq!(std::fs::read_to_string(&log).unwrap(), "after\n");
    }

    /// Rotation used to be decided by a stat of the path on every open; with
    /// a kept handle it has to be decided from the handle, or a log the app
    /// was already writing would grow past the cap for the rest of the run.
    #[test]
    fn a_rotation_while_the_handle_is_open_still_moves_the_old_file_aside() {
        let dir = TempDir::new("rotate-open");
        let log = dir.join(LOG_NAME);
        append(&log, "first\n").expect("open the handle");
        let over_cap = "x".repeat(MAX_BYTES as usize + 1);
        std::fs::write(&log, &over_cap).expect("grow the same file past the cap");
        append(&log, "after\n").expect("write past the cap");
        assert_eq!(std::fs::read_to_string(&log).unwrap(), "after\n");
        assert_eq!(std::fs::read_to_string(rotated(&log)).unwrap(), over_cap);
    }

    #[cfg(unix)]
    fn mode_of(path: &Path) -> u32 {
        use std::os::unix::fs::PermissionsExt;
        std::fs::metadata(path).expect("stat").permissions().mode() & 0o777
    }

    /// The log was the one file under the data directory any other account on
    /// the machine could read.
    #[cfg(unix)]
    #[test]
    fn the_log_is_created_readable_by_its_owner_alone() {
        let dir = TempDir::new("mode");
        let log = dir.join(LOG_NAME);
        append(&log, "line\n").expect("write");
        assert_eq!(mode_of(&log), 0o600, "{:o}", mode_of(&log));
    }

    #[cfg(unix)]
    #[test]
    fn a_log_left_world_readable_by_an_older_build_is_narrowed_on_open() {
        use std::os::unix::fs::PermissionsExt;
        let dir = TempDir::new("narrow");
        let log = dir.join(LOG_NAME);
        std::fs::write(&log, "old\n").expect("seed");
        std::fs::set_permissions(&log, std::fs::Permissions::from_mode(0o644)).expect("widen");
        assert_eq!(mode_of(&log), 0o644, "the seed is world-readable");
        append(&log, "new\n").expect("write");
        assert_eq!(mode_of(&log), 0o600, "{:o}", mode_of(&log));
        assert_eq!(std::fs::read_to_string(&log).unwrap(), "old\nnew\n");
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
    fn a_rotation_just_before_the_problem_does_not_hide_the_evening_that_explains_it() {
        // The bug: the log rotated at 8:39, the draft broke at 8:40, and the
        // dialog showed the two lines written since the rotation.
        let dir = TempDir::new("tail-rotated");
        let log = dir.join(LOG_NAME);
        std::fs::write(
            rotated(&log),
            "old 1\nold 2\nold 3\nold 4\nold 5\nold 6\nold 7\n",
        )
        .expect("seed the rotated generation");
        append(&log, "new 1\nnew 2\n").expect("write the current generation");

        // Older lines first, then the current ones, and never more than asked.
        assert_eq!(tail(&log, 4), vec!["old 6", "old 7", "new 1", "new 2"]);
        assert_eq!(
            tail(&log, 100),
            vec!["old 1", "old 2", "old 3", "old 4", "old 5", "old 6", "old 7", "new 1", "new 2",]
        );
    }

    #[test]
    fn a_current_generation_long_enough_on_its_own_is_not_joined_to_the_older_one() {
        let dir = TempDir::new("tail-enough");
        let log = dir.join(LOG_NAME);
        std::fs::write(rotated(&log), "old\n").expect("seed");
        append(&log, "a\nb\nc\n").expect("write");
        assert_eq!(tail(&log, 2), vec!["b", "c"]);
        assert_eq!(tail(&log, 3), vec!["a", "b", "c"]);
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
