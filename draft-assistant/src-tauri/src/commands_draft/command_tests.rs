//! Tests for the draft commands themselves: what a failure records, and the
//! shape of the paths that must not stall the app.
//!
//! Split out of `commands_draft.rs`, which is at the file-size cap; the
//! module is attached there with `#[path]`, in the style of `view.rs` and its
//! seat tests.

use super::*;

/// The stall this prevents: "Refresh projections" rebuilt the whole board
/// and then built the view under both mutexes, on a runtime thread, which
/// is exactly what the poll loop was rewritten to stop doing. Every
/// undrafted player is copied into a view, and for the length of that
/// copy the app is frozen.
///
/// Structural because the shape is the whole of the fix and there is no
/// Tauri `State` here to drive the command with: the refresh path must
/// hand a copy to `build_view_off_lock`, and must not call `view_from`
/// with the locks still in hand.
#[test]
fn the_full_refresh_builds_its_view_off_the_lock() {
    let source = include_str!("../commands_draft.rs");
    let body = source
        .split_once("async fn refresh_data_inner")
        .expect("refresh_data_inner is still the refresh path")
        .1
        .split_once("\n}\n")
        .expect("a closing brace")
        .0;
    assert!(
        body.contains("build_view_off_lock("),
        "the refresh builds its view under the locks again: {body}"
    );
    assert!(
        !body.contains("view_from("),
        "the refresh calls view_from with the locks held: {body}"
    );
}

/// The failure this prevents: a command returned `Err`, the string became
/// a toast, the toast was dismissed, and nothing anywhere recorded that
/// the command had been called at all.
#[test]
fn a_draft_command_that_fails_leaves_an_error_line_naming_it() {
    let (state, dir) = AppState::scratch("draft-log");
    // The same wrapper the `get_state` command is, with the Tauri `State`
    // it cannot have in a unit test taken out.
    let (out, lines) = crate::applog::captured(|| async {
        crate::applog::logged!(
            "get_state",
            ids(&state).await,
            get_state_inner(&state).await
        )
    });
    assert_eq!(
        out.unwrap_err(),
        "no league loaded",
        "the sentence the user sees is unchanged"
    );
    assert!(
        lines
            .iter()
            .any(|line| line.contains("ERROR get_state failed: no league loaded")),
        "{lines:?}"
    );
    let _ = std::fs::remove_dir_all(&dir);
}

#[tokio::test]
async fn a_failed_identity_save_does_not_change_the_active_roster() {
    let (state, dir) = AppState::scratch("identity-transaction");
    state.config.lock().await.my_user_id = Some("original".into());
    // A directory cannot be replaced by the settings file, even as root.
    std::fs::create_dir_all(dir.join("config.json")).unwrap();
    assert!(save_identity(&state, "replacement".into()).await.is_err());
    assert_eq!(
        state.config.lock().await.my_user_id.as_deref(),
        Some("original")
    );
    std::fs::remove_dir_all(dir).unwrap();
}

#[tokio::test]
async fn a_successful_identity_save_updates_memory_and_disk_together() {
    let (state, dir) = AppState::scratch("identity-success");
    save_identity(&state, "replacement".into()).await.unwrap();
    assert_eq!(
        state.config.lock().await.my_user_id.as_deref(),
        Some("replacement")
    );
    let stored: serde_json::Value =
        serde_json::from_slice(&std::fs::read(dir.join("config.json")).unwrap()).unwrap();
    assert_eq!(stored["my_user_id"], "replacement");
    std::fs::remove_dir_all(dir).unwrap();
}
