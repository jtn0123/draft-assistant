//! An append-only log, so a problem the user hit yesterday is still readable
//! today.
//!
//! Warnings used to go to stderr only, which in a bundled `.app` means they go
//! nowhere at all: double-clicking Draft Assistant gives it no terminal, so
//! every "projection source unreachable" or "cache write failed" was written
//! straight into the void. This writes them to `draft-assistant.log` beside
//! the app's other data instead.
//!
//! Four levels, because one was not enough to tell a failed Keychain write
//! from a missing weekly projection when reading the file back. The bottom one
//! is switchable while the app runs, from Settings -> Diagnostics: an
//! environment variable is no use to someone who double-clicks a bundled
//! `.app` on draft night.
//!
//! Everything goes through [`redact`] on the way in: the log's whole purpose
//! is to be pasted into a chat window on draft night, and a URL quoted back by
//! a failed request carries whatever was in its query string.
//!
//! Every failure here is swallowed: a logger that panics because the disk is
//! full turns a warning into a crash, which is strictly worse than a lost log
//! line.

#[cfg(test)]
mod capture;
mod file;
mod health;
mod redact;

#[cfg(test)]
pub(crate) use capture::{captured, Capture};

pub use file::{tail, LOG_NAME};
pub use health::HealthWatch;
pub use redact::redact;

use std::path::PathBuf;
use std::sync::atomic::{AtomicU8, Ordering};
use std::sync::OnceLock;

/// Where the log lives, once the app knows its data directory.
///
/// Set once from `lib.rs`'s `setup`. Anything that logs before that -- and
/// every unit test, every `cargo test`, both `dump_*` binaries -- falls back to
/// stderr, so nothing has to care whether the app is running.
static DIR: OnceLock<PathBuf> = OnceLock::new();

/// The environment variable that turns [`debug`] on.
///
/// Debug lines are the ones inside loops. Writing them by default would fill
/// the megabyte cap in an afternoon of polling and rotate away the warnings
/// that actually explain a draft night.
const DEBUG_VAR: &str = "DRAFT_ASSISTANT_DEBUG";

/// The two levels this app has a use for. `debug` adds the lines inside
/// loops; `info` is everything else, which is what a normal session writes.
pub const LEVEL_DEBUG: &str = "debug";
pub const LEVEL_INFO: &str = "info";

/// Nobody has chosen a level yet, so `DRAFT_ASSISTANT_DEBUG` decides.
const LEVEL_UNSET: u8 = 2;
const DEBUG_OFF: u8 = 0;
const DEBUG_ON: u8 = 1;

/// Whether debug lines are being written, as chosen at runtime.
///
/// The failure this exists to prevent: the only way to turn debug on used to
/// be an environment variable, which a user who double-clicks a bundled `.app`
/// has no way to set. Now Settings -> Diagnostics can, and the choice is
/// re-applied from the config at startup.
static DEBUG: AtomicU8 = AtomicU8::new(LEVEL_UNSET);

/// Turn debug lines on or off. Anything that is not `debug` means off, so a
/// config file carrying a level this app no longer knows is quiet rather than
/// noisy.
pub fn set_level(level: &str) {
    DEBUG.store(
        if level.eq_ignore_ascii_case(LEVEL_DEBUG) {
            DEBUG_ON
        } else {
            DEBUG_OFF
        },
        Ordering::Relaxed,
    );
}

/// The level in force, for the Diagnostics dialog's checkbox to reflect.
pub fn level() -> &'static str {
    if debug_wanted() {
        LEVEL_DEBUG
    } else {
        LEVEL_INFO
    }
}

/// `DEBUG` is one static for the whole test binary, so every test that moves
/// it takes a turn behind this rather than racing the others into a flake.
#[cfg(test)]
pub(crate) static LEVEL_GATE: std::sync::Mutex<()> = std::sync::Mutex::new(());

/// Put the level back where a fresh process would have it, so a test that
/// turned debug on cannot leave every later test writing debug lines.
#[cfg(test)]
pub(crate) fn reset_level_for_tests() {
    DEBUG.store(LEVEL_UNSET, Ordering::Relaxed);
}

/// The level a string names, or `None` when it names neither. The command
/// layer refuses the `None` rather than silently storing a level that does
/// nothing.
pub fn parse_level(level: &str) -> Option<&'static str> {
    if level.eq_ignore_ascii_case(LEVEL_DEBUG) {
        Some(LEVEL_DEBUG)
    } else if level.eq_ignore_ascii_case(LEVEL_INFO) {
        Some(LEVEL_INFO)
    } else {
        None
    }
}

/// Point the log at the app data directory. Later calls are ignored: the
/// directory does not change while the app runs, and a second call is a bug
/// worth ignoring rather than panicking over.
pub fn init(dir: PathBuf) {
    let _ = DIR.set(dir);
}

/// The file every line is going to, once the app knows. `None` before `init`,
/// which is every test and both dump binaries.
pub fn log_path() -> Option<PathBuf> {
    DIR.get().map(|dir| dir.join(LOG_NAME))
}

/// Something the user will notice: a command that failed, a panic, a league
/// that would not load.
pub fn error(msg: impl AsRef<str>) {
    write("ERROR", msg.as_ref());
}

/// Something went wrong that the app worked around.
pub fn warn(msg: impl AsRef<str>) {
    write("WARN", msg.as_ref());
}

/// A thing that happened, worth having in the timeline when reading the log
/// back: polling started, a league was switched.
pub fn info(msg: impl AsRef<str>) {
    write("INFO", msg.as_ref());
}

/// Detail for a problem being chased. Written only while the level is `debug`
/// -- set from Settings -> Diagnostics, or by `DRAFT_ASSISTANT_DEBUG` when
/// nobody has chosen -- so a call in a poll loop costs one atomic read per
/// tick and nothing else.
pub fn debug(msg: impl AsRef<str>) {
    if debug_wanted() {
        write("DEBUG", msg.as_ref());
    }
}

/// Whether debug lines are being written. Split out so the rule can be tested
/// without a log file: a debug call in a poll loop that wrote by default would
/// fill the megabyte cap in an afternoon and rotate the warnings away.
///
/// A runtime choice wins over the environment variable in both directions: a
/// user who unticked "Verbose logging" gets a quiet log even on a machine
/// where the variable happens to be exported.
fn debug_wanted() -> bool {
    match DEBUG.load(Ordering::Relaxed) {
        LEVEL_UNSET => std::env::var_os(DEBUG_VAR).is_some(),
        chosen => chosen == DEBUG_ON,
    }
}

/// The ` league=… draft=…` tail a call site attaches so a line can be tied to
/// what it was about.
///
/// Empty pairs are dropped rather than written as `league=`, because half the
/// call sites have an id and half do not, and a column of empty keys reads as
/// a bug in the logger.
pub fn context(pairs: &[(&str, &str)]) -> String {
    let mut out = String::new();
    for (key, value) in pairs {
        if value.is_empty() {
            continue;
        }
        out.push(' ');
        out.push_str(key);
        out.push('=');
        out.push_str(value);
    }
    out
}

/// A `map_err` for a command: log the failure with the command's name and
/// whatever ids were to hand, then hand the error on unchanged.
///
/// The error the user sees is not altered — the toast still says exactly what
/// it said before. The point is that after the toast is dismissed there is
/// still a record that the command was called and how it ended.
pub fn failing(command: &'static str, context: String) -> impl FnOnce(String) -> String {
    move |error: String| {
        self::error(format!("{command} failed: {error}{context}"));
        error
    }
}

/// Wrap a command's outcome: on failure log the command's name, the error and
/// whatever ids the caller can supply, then hand the error back untouched.
///
/// The failure this exists to prevent is the one that made the whole log worth
/// rewriting: a command returned `Err`, the string became a toast, the toast
/// was dismissed, and afterwards there was no record the command had even been
/// called. Every `#[tauri::command]` that can fail goes through this.
///
/// A macro rather than a plain function because building the context means
/// taking a lock, and that is only worth doing when something actually failed:
/// the expression here is evaluated on the error path and nowhere else.
macro_rules! logged {
    ($command:literal, $context:expr, $body:expr) => {
        match $body {
            Ok(value) => Ok(value),
            Err(error) => Err($crate::applog::failing($command, $context)(error)),
        }
    };
}
pub(crate) use logged;

/// Send every panic to the log before the default hook has its say.
///
/// Without this a panic in a bundled `.app` is completely silent: the process
/// dies, the window vanishes, and stderr went nowhere. Installed once, from
/// `lib.rs`'s `setup`.
pub fn install_panic_hook() {
    install_hook(error);
}

/// The half of [`install_panic_hook`] the tests can drive, with somewhere
/// other than the log file to send the note.
fn install_hook(sink: impl Fn(String) + Send + Sync + 'static) {
    let previous = std::panic::take_hook();
    std::panic::set_hook(Box::new(move |info| {
        let location = info
            .location()
            .map(|at| format!(" at {}:{}:{}", at.file(), at.line(), at.column()));
        // The version is on the line itself because a panic is often all
        // that is left of a session: the process is going away, so there may
        // be no INFO start line above it saying which build this was.
        sink(format!(
            "PANIC [{}] {}{}",
            env!("CARGO_PKG_VERSION"),
            info.payload()
                .downcast_ref::<&str>()
                .map(|s| (*s).to_string())
                .or_else(|| info.payload().downcast_ref::<String>().cloned())
                .unwrap_or_else(|| "panicked".to_string()),
            location.unwrap_or_default(),
        ));
        // The default hook still runs: a developer with a terminal open should
        // see exactly what they saw before this existed.
        previous(info);
    }));
}

/// One line: timestamp, level, redacted message.
fn write(level: &str, msg: &str) {
    let line = format!(
        "{} {level} {}\n",
        file::timestamp(file::now_secs()),
        redact(msg)
    );
    #[cfg(test)]
    if capture::intercept(&line) {
        return;
    }
    match log_path() {
        Some(path) => {
            if file::append(&path, &line).is_err() {
                // The log is the thing that broke, so stderr is all that is
                // left. Silently dropping it would hide the original problem.
                eprint!("{line}");
            }
        }
        None => eprint!("{line}"),
    }
}

#[cfg(test)]
mod tests;
