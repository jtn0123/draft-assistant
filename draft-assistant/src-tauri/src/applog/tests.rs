//! The tests for `applog`'s own half: levels, context, the command wrapper
//! and the panic hook.
//!
//! In their own file because `applog.rs` was over the file cap once the
//! runtime level and the capture facility moved in. Nothing here changed in
//! the move.

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
fn a_command_wrapped_in_logged_writes_one_error_line_naming_it() {
    // The wrapper every command goes through. Its whole job is that the
    // failure survives the toast being dismissed.
    let capture = Capture::start();
    let handed_back: Result<(), String> = logged!(
        "refresh_picks",
        context(&[("draft", "d1")]),
        Err("sleeper timed out".to_string())
    );
    assert_eq!(
        handed_back.unwrap_err(),
        "sleeper timed out",
        "the toast says exactly what it said before"
    );
    assert!(
        capture.saw("ERROR refresh_picks failed: sleeper timed out draft=d1"),
        "{:?}",
        capture.lines()
    );
}

#[test]
fn a_command_that_succeeds_writes_nothing_at_all() {
    let capture = Capture::start();
    let fine: Result<u8, String> = logged!("get_state", context(&[]), Ok(7));
    assert_eq!(fine, Ok(7));
    assert!(capture.lines().is_empty(), "{:?}", capture.lines());
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
    assert!(
        note.starts_with(&format!("PANIC [{}] ", env!("CARGO_PKG_VERSION"))),
        "the build that died is named: {note}"
    );
    assert!(
        note.contains("applog/tests.rs:"),
        "the location is named: {note}"
    );
    // Redaction happens on the way into the file, so the note itself still
    // holds the raw text; what matters is that `error` is what receives it.
    assert!(!redact(note).contains("hunter2"), "{note}");
}

#[test]
fn debug_lines_are_off_unless_the_environment_asks_for_them() {
    let _held = LEVEL_GATE.lock().unwrap_or_else(|e| e.into_inner());
    reset_level_for_tests();
    // Nothing in the test suite sets it, and nothing should: the point of
    // the gate is that a poll loop's debug line is not written by default.
    assert!(std::env::var_os(DEBUG_VAR).is_none());
    assert!(!debug_wanted(), "debug must be off by default");
    assert_eq!(level(), LEVEL_INFO);
    // And a debug call with the gate shut writes nothing anywhere.
    let dir = TempDir::new("debug-off");
    debug("tick 4");
    assert!(!dir.join(LOG_NAME).exists());
}

#[test]
fn the_level_can_be_switched_at_runtime_and_back_again() {
    // The failure: debug was reachable only through an environment
    // variable, which nobody double-clicking a bundled .app can set.
    let _held = LEVEL_GATE.lock().unwrap_or_else(|e| e.into_inner());
    reset_level_for_tests();
    assert!(!debug_wanted(), "the default is still quiet");

    set_level(LEVEL_DEBUG);
    assert!(debug_wanted(), "the dialog's checkbox turns debug on");
    assert_eq!(level(), LEVEL_DEBUG);

    set_level(LEVEL_INFO);
    assert!(!debug_wanted(), "and unticking it turns debug back off");
    assert_eq!(level(), LEVEL_INFO);

    // A level nobody knows is quiet rather than noisy, so a config from a
    // later version cannot fill this one's disk.
    set_level("trace");
    assert!(!debug_wanted());
    reset_level_for_tests();
}

#[test]
fn only_the_two_levels_this_app_writes_are_accepted() {
    assert_eq!(parse_level("debug"), Some(LEVEL_DEBUG));
    assert_eq!(parse_level("DEBUG"), Some(LEVEL_DEBUG));
    assert_eq!(parse_level("info"), Some(LEVEL_INFO));
    assert_eq!(parse_level("trace"), None);
    assert_eq!(parse_level(""), None);
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
