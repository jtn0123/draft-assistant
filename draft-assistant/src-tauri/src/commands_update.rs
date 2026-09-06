//! Settings -> "Check for updates".
//!
//! `tauri-plugin-updater` has been registered and granted since the release
//! workflow landed, and the release job attaches a signed `latest.json` to
//! every tagged release. Nothing called it: an installed copy could not learn
//! that a newer one existed short of the user opening GitHub. These two
//! commands are the whole of the in-app path: one asks the feed, the other
//! downloads, verifies and swaps the bundle, then relaunches.
//!
//! The plugin's errors are developer prose ("Could not fetch a valid release
//! JSON from the remote"). The row shows whatever comes back from here, so
//! every error is turned into one plain sentence before it leaves. The
//! mapping and the outcome shape are pure functions, tested in
//! `commands_update_tests.rs`; the two commands are wrappers around them.

use crate::applog;
use tauri::{AppHandle, Runtime};
use tauri_plugin_updater::{Error as UpdaterError, Update, UpdaterExt};

/// What a check found. `available` is `None` when the installed copy is the
/// newest release the feed knows about.
#[derive(Debug, Clone, PartialEq, Eq, serde::Serialize)]
pub struct UpdateCheck {
    /// The version this copy runs, as the shell reports it.
    pub current: String,
    /// The newer version on the feed, if there is one.
    pub available: Option<String>,
    /// The release notes attached to that version, when the feed carried any.
    pub notes: Option<String>,
}

/// The one sentence the row shows for an updater failure.
///
/// Two of these are the states a fresh install will actually meet: no signed
/// release has been published yet, so `latest.json` is a 404 (the plugin
/// reports that as `ReleaseNotFound`), and the machine is offline (a
/// `reqwest` transport error). Both used to read as internal error text.
pub fn describe(error: &UpdaterError) -> String {
    match error {
        UpdaterError::ReleaseNotFound => {
            "No release feed yet. Nothing has been published to update to".to_string()
        }
        UpdaterError::Reqwest(_) | UpdaterError::Network(_) => {
            "Could not reach the update server. Check the connection and try again".to_string()
        }
        UpdaterError::EmptyEndpoints => {
            "This build has no update feed configured, so it cannot check".to_string()
        }
        UpdaterError::Minisign(_) | UpdaterError::SignatureUtf8(_) | UpdaterError::Base64(_) => {
            "The download did not match its signature, so it was not installed".to_string()
        }
        UpdaterError::TargetNotFound(_) | UpdaterError::TargetsNotFound(_) => {
            "The latest release has no build for this Mac".to_string()
        }
        UpdaterError::Serialization(_) | UpdaterError::Semver(_) => {
            "The update feed could not be read. Try again after the next release".to_string()
        }
        UpdaterError::Io(_) => {
            "Could not write the update to disk. Check the space left on this Mac".to_string()
        }
        other => format!("Update failed: {other}"),
    }
}

/// The outcome shape, from what the plugin's `check` handed back.
pub fn outcome(current: &str, found: Option<(&str, Option<&str>)>) -> UpdateCheck {
    UpdateCheck {
        current: current.to_string(),
        available: found.map(|(version, _)| version.to_string()),
        notes: found.and_then(|(_, notes)| notes.map(str::to_string)),
    }
}

/// Ask the feed in `tauri.conf.json` whether a newer signed release exists.
#[tauri::command]
pub async fn check_for_update<R: Runtime>(app: AppHandle<R>) -> Result<UpdateCheck, String> {
    applog::logged!("check_for_update", String::new(), check_on(&app).await)
}

async fn check_on<R: Runtime>(app: &AppHandle<R>) -> Result<UpdateCheck, String> {
    let current = app.package_info().version.to_string();
    let found = fetch(app).await?;
    let check = outcome(
        &current,
        found
            .as_ref()
            .map(|update| (update.version.as_str(), update.body.as_deref())),
    );
    match &check.available {
        Some(version) => applog::info(format!("update available: {current} -> {version}")),
        None => applog::debug(format!("update check: {current} is current")),
    }
    Ok(check)
}

/// Download the newer release, verify it against the public key, swap the
/// bundle and relaunch. Only ever returns on failure: success restarts.
#[tauri::command]
pub async fn install_update<R: Runtime>(app: AppHandle<R>) -> Result<(), String> {
    applog::logged!("install_update", String::new(), install_on(&app).await)
}

async fn install_on<R: Runtime>(app: &AppHandle<R>) -> Result<(), String> {
    let update = fetch(app)
        .await?
        .ok_or_else(|| "Nothing to install: this is the latest release".to_string())?;
    applog::info(format!("installing update {}", update.version));
    update
        .download_and_install(|_, _| {}, || {})
        .await
        .map_err(|error| describe(&error))?;
    applog::info(format!("update {} installed, restarting", update.version));
    app.restart()
}

async fn fetch<R: Runtime>(app: &AppHandle<R>) -> Result<Option<Update>, String> {
    let updater = app.updater().map_err(|error| describe(&error))?;
    updater.check().await.map_err(|error| describe(&error))
}

#[cfg(test)]
#[path = "commands_update_tests.rs"]
mod tests;
