//! A log the user can find after the fact.
//!
//! A packaged `.app` sends stderr nowhere, so until now a poll failure was a
//! pill colour, a cache fallback was a banner, and a chat stream that died
//! mid-answer left nothing behind at all. If the board freezes at pick 40
//! there has to be something to read afterwards.
//!
//! Deliberately dependency-free and Tauri-free: the domain crate owns it, so
//! `dump_state`, the tests and the desktop app all log the same way. Nothing
//! here can fail loudly — a logger that panics or propagates errors would be
//! worse than no logger — so every failure degrades to stderr.

use std::fmt::Display;
use std::fs::{File, OpenOptions};
use std::io::Write;
use std::path::{Path, PathBuf};
use std::sync::{Mutex, OnceLock};

/// Roll over at this size so a long draft cannot fill the disk. One previous
/// file is kept: enough to cover a session that has just rolled.
const MAX_BYTES: u64 = 4 * 1024 * 1024;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Level {
    Info,
    Warn,
}

impl Level {
    fn label(self) -> &'static str {
        match self {
            Level::Info => "INFO",
            Level::Warn => "WARN",
        }
    }
}

struct Sink {
    file: Option<File>,
    path: PathBuf,
}

static SINK: OnceLock<Mutex<Sink>> = OnceLock::new();

/// `HH:MM:SSZ` followed by the raw epoch seconds.
///
/// The clock is UTC — marked `Z` so nobody reads it as local — because
/// timezones need a date crate and this file is deliberately dependency-free.
/// The epoch seconds beside it are what you actually correlate with: they
/// match `data_health` in an exported state dump and Sleeper's own stamps.
fn stamp() -> String {
    let secs = crate::engine::now_secs();
    let day = secs % 86_400;
    format!(
        "{:02}:{:02}:{:02}Z {}",
        day / 3600,
        (day % 3600) / 60,
        day % 60,
        secs
    )
}

/// Point the log at `dir`, creating it if need be, and roll the file if it
/// has grown past [`MAX_BYTES`]. Safe to call more than once; the first call
/// wins, which keeps a stray second call in a test from moving the file.
pub fn init(dir: &Path) {
    let path = dir.join("draft-assistant.log");
    if let Err(error) = std::fs::create_dir_all(dir) {
        eprintln!("log: cannot create {}: {error}", dir.display());
    }
    roll_if_large(&path);
    let file = OpenOptions::new()
        .create(true)
        .append(true)
        .open(&path)
        .map_err(|error| eprintln!("log: cannot open {}: {error}", path.display()))
        .ok();
    let sink = Mutex::new(Sink { file, path });
    if SINK.set(sink).is_err() {
        return;
    }
    info(format!("--- log opened at {} ---", dir.display()));
}

fn roll_if_large(path: &Path) {
    let too_big = std::fs::metadata(path)
        .map(|m| m.len() > MAX_BYTES)
        .unwrap_or(false);
    if too_big {
        let _ = std::fs::rename(path, path.with_extension("log.1"));
    }
}

/// Where the log is, once [`init`] has run.
pub fn path() -> Option<PathBuf> {
    let sink = SINK.get()?.lock().ok()?;
    Some(sink.path.clone())
}

fn write(level: Level, message: &str) {
    let line = format!("{} {} {message}\n", stamp(), level.label());
    // Always to stderr: that is what `tauri dev` and `dump_state` show.
    eprint!("{line}");
    let Some(sink) = SINK.get() else { return };
    let Ok(mut sink) = sink.lock() else { return };
    if let Some(file) = sink.file.as_mut() {
        let _ = file.write_all(line.as_bytes());
    }
}

pub fn info(message: impl Display) {
    write(Level::Info, &message.to_string());
}

pub fn warn(message: impl Display) {
    write(Level::Warn, &message.to_string());
}

/// Log each of `warnings` once, prefixed so they can be grepped together.
/// The view carries these to the UI as banners; the log is where they persist.
pub fn warnings(context: &str, warnings: &[String]) {
    for warning in warnings {
        warn(format!("{context}: {warning}"));
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_line_carries_its_level_and_message() {
        let dir = std::env::temp_dir().join(format!("da-log-{}", std::process::id()));
        std::fs::create_dir_all(&dir).unwrap();
        let path = dir.join("draft-assistant.log");
        let _ = std::fs::remove_file(&path);

        // `init` is process-global and another test may have claimed it, so
        // exercise the formatting directly rather than fighting over the sink.
        let line = format!("{} {} {}\n", stamp(), Level::Warn.label(), "picks failed");
        assert!(line.contains("WARN picks failed"), "{line}");
        assert!(line.starts_with(char::is_numeric), "{line}");
        std::fs::remove_dir_all(&dir).ok();
    }

    #[test]
    fn an_oversized_log_is_rolled_aside_and_a_small_one_is_kept() {
        let dir = std::env::temp_dir().join(format!("da-log-roll-{}", std::process::id()));
        std::fs::create_dir_all(&dir).unwrap();
        let path = dir.join("draft-assistant.log");

        std::fs::write(&path, "small").unwrap();
        roll_if_large(&path);
        assert_eq!(std::fs::read_to_string(&path).unwrap(), "small");
        assert!(!path.with_extension("log.1").exists());

        std::fs::write(&path, vec![b'x'; (MAX_BYTES + 1) as usize]).unwrap();
        roll_if_large(&path);
        assert!(!path.exists(), "the oversized file moved aside");
        assert!(path.with_extension("log.1").exists());

        std::fs::remove_dir_all(&dir).ok();
    }

    #[test]
    fn levels_are_labelled_for_grepping() {
        assert_eq!(Level::Info.label(), "INFO");
        assert_eq!(Level::Warn.label(), "WARN");
    }
}
