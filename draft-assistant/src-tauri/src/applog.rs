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
//! from a missing weekly projection when reading the file back. Everything
//! goes through [`redact`] on the way in: the log's whole purpose is to be
//! pasted into a chat window on draft night, and a URL quoted back by a failed
//! request carries whatever was in its query string.
//!
//! Every failure here is swallowed: a logger that panics because the disk is
//! full turns a warning into a crash, which is strictly worse than a lost log
//! line.

mod file;
mod health;
mod redact;

pub use file::{tail, LOG_NAME};
pub use health::HealthWatch;
pub use redact::redact;

use std::path::PathBuf;
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

/// Detail for a problem being chased. Written only when `DRAFT_ASSISTANT_DEBUG`
/// is set, so a call in a poll loop costs one environment read per tick and
/// nothing else.
pub fn debug(msg: impl AsRef<str>) {
    if debug_wanted() {
        write("DEBUG", msg.as_ref());
    }
}

/// Whether debug lines are being written. Split out so the rule can be tested
/// without a log file: a debug call in a poll loop that wrote by default would
/// fill the megabyte cap in an afternoon and rotate the warnings away.
fn debug_wanted() -> bool {
    std::env::var_os(DEBUG_VAR).is_some()
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
        sink(format!(
            "PANIC {}{}",
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
mod tests {
    use super::*;
    use file::TempDir;

    /// `write` reads the process-wide `DIR`, which no test may set. These
    /// assertions are made against the same formatting through `append`.
    fn line_of(level: &str, msg: &str) -> String {
        format!("{} {level} {}\n", file::timestamp(0), redact(msg))
    }

    #[test]
    fn every_level_writes_its_own_prefix_so_the_file_can_be_read_by_severity() {
        let dir = TempDir::new("levels");
        let log = dir.join(LOG_NAME);
        for (level, msg) in [
            ("ERROR", "could not load the league"),
            ("WARN", "projection source unreachable"),
            ("INFO", "polling started"),
            ("DEBUG", "tick 4"),
        ] {
            file::append(&log, &line_of(level, msg)).expect("write");
        }
        let text = std::fs::read_to_string(&log).unwrap();
        assert!(text.contains(" ERROR could not load the league"));
        assert!(text.contains(" WARN projection source unreachable"));
        assert!(text.contains(" INFO polling started"));
        assert!(text.contains(" DEBUG tick 4"));
    }

    #[test]
    fn a_secret_quoted_back_by_a_failed_request_never_reaches_the_file() {
        let dir = TempDir::new("redact");
        let log = dir.join(LOG_NAME);
        file::append(
            &log,
            &line_of("ERROR", "POST /token?client_secret=hunter2 refused"),
        )
        .expect("write");
        let text = std::fs::read_to_string(&log).unwrap();
        assert!(!text.contains("hunter2"), "{text}");
        assert!(text.contains("client_secret=····"), "{text}");
    }

    #[test]
    fn context_names_the_ids_and_skips_the_ones_that_are_missing() {
        assert_eq!(
            context(&[("league", "123"), ("draft", "456")]),
            " league=123 draft=456"
        );
        assert_eq!(context(&[("league", "123"), ("draft", "")]), " league=123");
        assert_eq!(context(&[]), "");
    }

    #[test]
    fn a_failing_command_hands_its_error_back_exactly_as_it_was_given() {
        // The user-visible sentence must not change: the toast is the same
        // toast, and only the log gains a line.
        let handed_back = failing("add_league", context(&[("league", "123")]))(
            "no league 123 on your account".to_string(),
        );
        assert_eq!(handed_back, "no league 123 on your account");
    }

    #[test]
    fn a_panic_reaches_the_hook_with_its_message_and_where_it_happened() {
        use std::sync::{Arc, Mutex};
        let seen: Arc<Mutex<Vec<String>>> = Arc::new(Mutex::new(Vec::new()));
        let sink = seen.clone();
        install_hook(move |note| sink.lock().expect("sink lock").push(note));

        let panicked = std::panic::catch_unwind(|| panic!("token=hunter2 was refused"));
        assert!(panicked.is_err(), "the panic still propagates");

        // Back to the default hook before anything else in this binary
        // panics, so this test's sink does not outlive it.
        let _ = std::panic::take_hook();

        let notes = seen.lock().expect("sink lock");
        let note = notes.first().expect("the hook wrote a note");
        assert!(note.starts_with("PANIC "), "{note}");
        assert!(note.contains("applog.rs:"), "the location is named: {note}");
        // Redaction happens on the way into the file, so the note itself still
        // holds the raw text; what matters is that `error` is what receives it.
        assert!(!redact(note).contains("hunter2"), "{note}");
    }

    #[test]
    fn debug_lines_are_off_unless_the_environment_asks_for_them() {
        // Nothing in the test suite sets it, and nothing should: the point of
        // the gate is that a poll loop's debug line is not written by default.
        assert!(std::env::var_os(DEBUG_VAR).is_none());
        assert!(!debug_wanted(), "debug must be off by default");
        // And a debug call with the gate shut writes nothing anywhere.
        let dir = TempDir::new("debug-off");
        debug("tick 4");
        assert!(!dir.join(LOG_NAME).exists());
    }

    #[test]
    fn logging_before_init_writes_nothing_to_disk_and_does_not_panic() {
        // No `init` has run in this test binary, so `DIR` is empty and the
        // fallback is stderr. The assertion that matters is that this neither
        // panics nor creates a file anywhere the test can see.
        let dir = TempDir::new("preinit");
        warn("engine could not reach the projection source");
        error("and this one too");
        assert!(
            !dir.join(LOG_NAME).exists(),
            "a pre-init log call must not invent a log file"
        );
        assert!(DIR.get().is_none(), "no test in this binary may call init");
        assert_eq!(log_path(), None);
    }
}
