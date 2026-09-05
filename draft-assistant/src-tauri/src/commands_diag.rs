//! Settings -> "Diagnostics…", and the frontend's error reporter.
//!
//! The failure this exists to prevent is the draft-night one: something goes
//! wrong, and neither the user nor anyone reading over their shoulder can say
//! what. The log has been written since the app was built, but nothing ever
//! told anyone where it was, and a page-level error -- a rejected promise, a
//! screen that would not render -- never reached it at all.
//!
//! Three commands. `diagnostics` is everything worth pasting, `log_frontend_error`
//! is the webview's way into the same log, and `open_log_folder` puts the file
//! in front of the user in their file manager.

use crate::applog;
use crate::companion::CompanionServer;
use crate::poll::{poll_health, PollHealth};
use crate::state::AppState;
use std::sync::atomic::Ordering;
use std::sync::Arc;
use tauri::State;

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
        log_tail,
    })
}

/// The webview's way into the log: a render error, a rejected promise, or
/// anything `window.onerror` caught.
///
/// Always `Ok`. A reporter that can fail is a reporter that reports its own
/// failure, and the frontend has no way to tell a real problem from that loop.
#[tauri::command]
pub async fn log_frontend_error(message: String, source: Option<String>) -> Result<(), String> {
    applog::error(frontend_line(&message, source.as_deref()));
    Ok(())
}

/// The line a page-level failure becomes. Split out so a test can assert what
/// is stored without a log file to read back.
fn frontend_line(message: &str, source: Option<&str>) -> String {
    format!(
        "frontend: {message}{}",
        applog::context(&[("where", source.unwrap_or(""))])
    )
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
                Some("unhandledrejection")
            ),
            "frontend: Cannot read properties of undefined where=unhandledrejection"
        );
        // A page that cannot say where is still worth storing.
        assert_eq!(frontend_line("boom", None), "frontend: boom");
    }

    #[test]
    fn a_frontend_error_that_quotes_a_url_is_masked_before_it_is_stored() {
        // The page's own error strings quote whatever URL failed, which on a
        // follower is the host's address with its bearer token in it.
        let line = frontend_line("GET /api/state?token=abc123 failed", Some("render"));
        assert!(!applog::redact(&line).contains("abc123"), "{line}");
    }

    #[tokio::test]
    async fn the_reporting_command_never_fails_whatever_the_page_hands_it() {
        // A reporter that can fail is one the frontend has to report about.
        assert!(log_frontend_error(String::new(), None).await.is_ok());
    }
}
