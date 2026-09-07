//! Settings -> "Diagnostics…", and the frontend's error reporter.
//!
//! The failure this exists to prevent is the draft-night one: something goes
//! wrong, and neither the user nor anyone reading over their shoulder can say
//! what. The log has been written since the app was built, but nothing ever
//! told anyone where it was, and a page-level error -- a rejected promise, a
//! screen that would not render -- never reached it at all.
//!
//! Four commands. `diagnostics` is everything worth pasting, `log_frontend_error`
//! is the webview's way into the same log, `open_log_folder` puts the file in
//! front of the user in their file manager, and `set_log_level` is the only
//! way a user without a terminal can turn verbose logging on.

use crate::applog;
use crate::companion::CompanionServer;
use crate::engine::{AppConfig, Engine};
use crate::poll::{poll_health, PollHealth};
use crate::state::AppState;
use std::sync::atomic::Ordering;
use std::sync::Arc;
use tauri::State;
use tokio::sync::Mutex;

/// Everything the Diagnostics dialog shows, and everything "Copy diagnostics"
/// puts on the clipboard.
///
/// Deliberately no pairing code and no token: this is built to be pasted into
/// a chat window, and the whole of the companion's security is six digits.
#[derive(serde::Serialize)]
pub struct Diagnostics {
    pub app_version: String,
    /// `macos aarch64` — enough to tell two bug reports apart.
    pub platform: String,
    pub league_id: Option<String>,
    pub league_name: Option<String>,
    pub draft_id: Option<String>,
    /// Which service the league on screen is read from, when there is one.
    pub platform_name: Option<String>,
    pub polling: bool,
    pub poll: Option<PollHealth>,
    pub companion_enabled: bool,
    /// How many devices are paired. The devices themselves are on the
    /// companion panel; a count is all this needs.
    pub companion_devices: usize,
    /// Where the log is, or `None` before the app has a data directory —
    /// which in practice means only the tests.
    pub log_path: Option<String>,
    /// `"debug"` or `"info"`: what the "Verbose logging" checkbox shows.
    pub log_level: String,
    pub log_tail: Vec<String>,
}

/// How many lines of the log the dialog shows. Two hundred is a few minutes of
/// a bad draft night and still small enough to read and to paste.
const TAIL_LINES: usize = 200;

#[tauri::command]
pub async fn diagnostics(
    state: State<'_, AppState>,
    companion: State<'_, Arc<CompanionServer>>,
) -> Result<Diagnostics, String> {
    let log_path = applog::log_path();
    let log_tail = log_path
        .as_ref()
        .map(|path| applog::tail(path, TAIL_LINES))
        .unwrap_or_default();
    let loaded = state.loaded.lock().await;
    let league = loaded.as_ref();
    Ok(Diagnostics {
        app_version: env!("CARGO_PKG_VERSION").to_string(),
        platform: format!("{} {}", std::env::consts::OS, std::env::consts::ARCH),
        league_id: league.map(|l| l.league.league_id.clone()),
        league_name: league.map(|l| l.league.name.clone()),
        draft_id: league.map(|l| l.draft.draft_id.clone()),
        platform_name: league
            .map(|l| crate::view_types::platform_for(&l.league.league_id).to_string()),
        polling: state.polling.load(Ordering::SeqCst),
        poll: league.map(poll_health),
        companion_enabled: companion.is_enabled(),
        companion_devices: companion.hub.devices().len(),
        log_path: log_path.map(|path| path.to_string_lossy().to_string()),
        log_level: applog::level().to_string(),
        log_tail,
    })
}

/// Turn verbose logging on or off from the Diagnostics dialog, and remember
/// the choice.
///
/// The failure this exists to prevent: debug lines were reachable only by
/// exporting `DRAFT_ASSISTANT_DEBUG` before launch, which is not a thing a
/// user who double-clicks the app on draft night can do. Applied immediately
/// and again at startup from the config.
#[tauri::command]
pub async fn set_log_level(state: State<'_, AppState>, level: String) -> Result<String, String> {
    applog::logged!(
        "set_log_level",
        String::new(),
        set_level_on(&state.engine, &state.config, &level).await
    )
}

/// The half of `set_log_level` with no Tauri `State` in it, so a test can
/// drive it with an engine pointed at a scratch directory.
async fn set_level_on(
    engine: &Engine,
    config_ref: &Mutex<AppConfig>,
    level: &str,
) -> Result<String, String> {
    let level = applog::parse_level(level)
        .ok_or_else(|| format!("{level:?} is not a log level this app writes"))?;
    let mut config = config_ref.lock().await;
    let previous = config.log_level.take();
    config.log_level = Some(level.to_string());
    // Saved rather than only held: chasing a problem usually means restarting,
    // and a verbose setting that resets on restart is no use for that. Saved
    // before it is applied, too: a save that fails leaves the level where it
    // was, so what the checkbox shows and what the file says never disagree.
    if let Err(why) = engine.save_config(&config) {
        config.log_level = previous;
        return Err(why);
    }
    applog::set_level(level);
    applog::info(format!("log level set to {level}"));
    Ok(level.to_string())
}

/// The webview's way into the log: a render error, a rejected promise, or
/// anything `window.onerror` caught.
///
/// Always `Ok`. A reporter that can fail is a reporter that reports its own
/// failure, and the frontend has no way to tell a real problem from that loop.
#[tauri::command]
pub async fn log_frontend_error(
    message: String,
    source: Option<String>,
    stack: Option<String>,
) -> Result<(), String> {
    applog::error(frontend_line(&message, source.as_deref(), stack.as_deref()));
    Ok(())
}

/// The line a page-level failure becomes. Split out so a test can assert what
/// is stored without a log file to read back.
///
/// The stack is the page's first frames, on the same line: the log is read a
/// line at a time, and "TypeError: undefined is not a function" with no frame
/// named nothing about which screen had failed.
fn frontend_line(message: &str, source: Option<&str>, stack: Option<&str>) -> String {
    format!(
        "frontend: {message}{}",
        applog::context(&[
            ("where", source.unwrap_or("")),
            ("stack", &one_line(stack.unwrap_or(""))),
        ])
    )
}

/// A stack's frames joined onto one line, innermost first.
fn one_line(stack: &str) -> String {
    stack
        .lines()
        .map(str::trim)
        .filter(|frame| !frame.is_empty())
        .collect::<Vec<_>>()
        .join(" <- ")
}

/// Show the log's folder in the user's file manager, and hand back the path
/// either way so the dialog can offer it to be copied.
///
/// No plugin: `tauri-plugin-opener` is not a dependency of this app and one
/// process spawn is not worth adding it for. If the spawn fails — a stripped
/// container, a locked-down machine — the path still comes back, which is the
/// half that matters.
#[tauri::command]
pub async fn open_log_folder() -> Result<String, String> {
    applog::logged!("open_log_folder", String::new(), show_log_folder())
}

/// The half with the error in it, so the wrapper above is only the wrapper.
fn show_log_folder() -> Result<String, String> {
    let path = applog::log_path().ok_or("this app has no log file yet")?;
    let folder = path.parent().unwrap_or(&path).to_path_buf();
    let shown = folder.to_string_lossy().to_string();
    if let Some(opener) = file_manager() {
        // Detached on purpose: nothing here waits for a file manager to be
        // closed, and a status code from `open` would say nothing useful.
        match std::process::Command::new(opener).arg(&folder).spawn() {
            Ok(_) => {}
            Err(e) => applog::warn(format!("could not open the log folder: {e}")),
        }
    }
    Ok(shown)
}

/// The command that shows a folder, per platform. `None` where there is no
/// obvious one, in which case the caller falls back to showing the path.
fn file_manager() -> Option<&'static str> {
    match std::env::consts::OS {
        "macos" => Some("open"),
        "windows" => Some("explorer"),
        "linux" => Some("xdg-open"),
        _ => None,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn every_desktop_this_app_ships_to_has_a_way_to_show_a_folder() {
        // The dialog's "Open log folder" button is hidden on a platform with
        // no opener, so this is what decides whether it is offered at all.
        assert!(
            file_manager().is_some(),
            "no file manager known for {}",
            std::env::consts::OS
        );
    }

    #[test]
    fn a_frontend_error_is_stored_with_where_it_came_from() {
        assert_eq!(
            frontend_line(
                "Cannot read properties of undefined",
                Some("unhandledrejection"),
                None
            ),
            "frontend: Cannot read properties of undefined where=unhandledrejection"
        );
        // A page that cannot say where is still worth storing.
        assert_eq!(frontend_line("boom", None, None), "frontend: boom");
    }

    /// The failure this prevents: a render error reached the log as
    /// "frontend: TypeError: x is undefined where=render", which names no
    /// screen and no component, so the log could not say what had failed.
    #[test]
    fn a_frontend_error_keeps_its_first_frames_on_the_same_line() {
        let line = frontend_line(
            "TypeError: x is undefined",
            Some("render"),
            Some("    at Board (http://localhost/assets/index.js:10:5)\n    at Panel\n"),
        );
        assert_eq!(
            line,
            "frontend: TypeError: x is undefined where=render \
             stack=at Board (http://localhost/assets/index.js:10:5) <- at Panel"
        );
        assert!(!line.contains('\n'), "one entry is one line of the log");
    }

    #[test]
    fn a_frontend_error_that_quotes_a_url_is_masked_before_it_is_stored() {
        // The page's own error strings quote whatever URL failed, which on a
        // follower is the host's address with its bearer token in it.
        let line = frontend_line("GET /api/state?token=abc123 failed", Some("render"), None);
        assert!(!applog::redact(&line).contains("abc123"), "{line}");
    }

    /// A scratch data directory, so no test ever writes a real config.
    fn scratch(tag: &str) -> std::path::PathBuf {
        let dir =
            std::env::temp_dir().join(format!("draft-assistant-diag-{tag}-{}", std::process::id()));
        std::fs::create_dir_all(&dir).expect("create scratch dir");
        dir
    }

    /// Drive an async body while holding the process-wide level gate.
    ///
    /// A plain `fn` with its own runtime rather than a `#[tokio::test]`: the
    /// gate is a blocking mutex, and holding one of those across an `.await`
    /// is the deadlock `clippy::await_holding_lock` exists to stop.
    fn with_level_gate<F: std::future::Future>(body: impl FnOnce() -> F) -> F::Output {
        let _held = applog::LEVEL_GATE.lock().unwrap_or_else(|e| e.into_inner());
        applog::reset_level_for_tests();
        let runtime = tokio::runtime::Builder::new_current_thread()
            .enable_all()
            .build()
            .expect("a runtime for the test");
        let out = runtime.block_on(body());
        applog::reset_level_for_tests();
        out
    }

    #[test]
    fn turning_verbose_logging_on_is_applied_now_and_remembered_for_next_time() {
        // The failure: debug lines could only be had by exporting an
        // environment variable before launch, which a user who double-clicks
        // the app cannot do.
        let dir = scratch("level");
        with_level_gate(|| async {
            let engine = Engine::new(dir.clone());
            let config = Mutex::new(AppConfig::default());

            let chosen = set_level_on(&engine, &config, "debug")
                .await
                .expect("debug is a level this app writes");
            assert_eq!(chosen, applog::LEVEL_DEBUG);
            assert_eq!(applog::level(), applog::LEVEL_DEBUG, "applied immediately");
            assert_eq!(
                config.lock().await.log_level.as_deref(),
                Some(applog::LEVEL_DEBUG),
                "and stored, so a restart to reproduce the problem is still verbose"
            );

            set_level_on(&engine, &config, "info")
                .await
                .expect("and back off again");
            assert_eq!(applog::level(), applog::LEVEL_INFO);
        });
        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn a_level_this_app_does_not_write_is_refused_rather_than_stored() {
        let dir = scratch("level-bad");
        with_level_gate(|| async {
            let engine = Engine::new(dir.clone());
            let config = Mutex::new(AppConfig::default());
            let refused = set_level_on(&engine, &config, "trace").await;
            assert!(refused.is_err(), "a level that does nothing is not stored");
            assert_eq!(config.lock().await.log_level, None);
        });
        let _ = std::fs::remove_dir_all(&dir);
    }

    /// The failure this prevents: the level was switched in memory before
    /// the config was written, so a save that failed left the app verbose
    /// with a file that still said quiet, and the next launch disagreed with
    /// the checkbox the user had just ticked.
    #[test]
    fn a_level_whose_save_fails_is_not_applied_either() {
        let dir = scratch("level-unsaved");
        // A data directory that is a file: nothing under it can be written.
        let blocked = dir.join("not-a-directory");
        std::fs::write(&blocked, b"in the way").expect("a file where the dir would be");
        with_level_gate(|| async {
            let engine = Engine::new(blocked.clone());
            let config = Mutex::new(AppConfig::default());
            let refused = set_level_on(&engine, &config, "debug").await;
            assert!(refused.is_err(), "the save cannot have succeeded");
            assert_eq!(applog::level(), applog::LEVEL_INFO, "not applied");
            assert_eq!(config.lock().await.log_level, None, "not held either");
        });
        let _ = std::fs::remove_dir_all(&dir);
    }

    #[tokio::test]
    async fn the_reporting_command_never_fails_whatever_the_page_hands_it() {
        // A reporter that can fail is one the frontend has to report about.
        assert!(log_frontend_error(String::new(), None, None).await.is_ok());
    }
}
