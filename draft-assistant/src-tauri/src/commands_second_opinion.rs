//! The Settings row "Import projections CSV…" — the one command behind it.
//!
//! The native file picker is opened here, in Rust, rather than from the page.
//! The dialog plugin's own commands are therefore not granted in
//! `capabilities/default.json`: the frontend asks the *app* to import a file
//! and the app is what chooses one, so a compromised page cannot open a
//! picker, and cannot name a path either.

use crate::second_opinion::{self, MatchReport, SECOND_OPINION_FILE};
use crate::state::{view_from, AppState};
use crate::view::DraftView;
use serde::Serialize;
use std::path::PathBuf;
use tauri::State;
use tauri_plugin_dialog::DialogExt;

/// What the Settings row gets back: the two numbers for the toast, and the
/// rebuilt view so the new column appears without a second round trip.
#[derive(Debug, Clone, Serialize)]
pub struct SecondOpinionImport {
    pub matched: usize,
    pub total: usize,
    /// The sentence the toast shows, written once, in Rust.
    pub message: String,
    pub view: DraftView,
}

/// Ask the user for a file. `None` when they closed the picker.
async fn pick_file(app: &tauri::AppHandle) -> Option<PathBuf> {
    let (tx, rx) = tokio::sync::oneshot::channel();
    app.dialog()
        .file()
        .add_filter("Projections CSV", &["csv"])
        .set_title("Import projections CSV")
        .pick_file(move |chosen| {
            // The receiver is only dropped if the command itself was
            // cancelled, in which case nobody wants the answer.
            let _ = tx.send(chosen);
        });
    rx.await
        .ok()
        .flatten()
        .and_then(|path| path.into_path().ok())
}

/// Open the picker, parse what was chosen, keep a copy, and re-stamp the
/// board. Returns `None` when the user cancelled — not an error.
#[tauri::command]
pub async fn import_second_opinion(
    app: tauri::AppHandle,
    state: State<'_, AppState>,
) -> Result<Option<SecondOpinionImport>, String> {
    let Some(source) = pick_file(&app).await else {
        return Ok(None);
    };
    let text = std::fs::read_to_string(&source)
        .map_err(|e| format!("that file could not be opened: {e}"))?;
    let loaded_at = crate::engine::now_secs();
    // Parsed before it is kept, so a file that is not a projections export
    // never displaces the one that already works.
    let table = second_opinion::parse(&text, loaded_at)?;
    // Kept through the same temp-then-rename the caches use, so an import
    // interrupted half way leaves the previous file intact.
    let destination = state.engine.data_dir.join(SECOND_OPINION_FILE);
    let tmp = destination.with_extension("csv.tmp");
    crate::cache::replace_file(tmp, destination, text.clone())
        .map_err(|e| format!("the imported file could not be saved: {e}"))?;

    let mut loaded = state.loaded.lock().await;
    let loaded = loaded.as_mut().ok_or("no league loaded")?;
    let report: MatchReport = second_opinion::apply(&table, &mut loaded.board);
    loaded.second_opinion_loaded_at = Some(loaded_at);
    let config = state.config.lock().await;
    let view = view_from(loaded, &config);
    Ok(Some(SecondOpinionImport {
        matched: report.matched,
        total: report.total,
        message: report.message(),
        view,
    }))
}
