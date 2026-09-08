//! The watcher, against a real Tauri event bus.
//!
//! This is the one link in the streaming chain with nothing behind it but the
//! window: `chat.rs` hands the text over, this puts it on the bus, and
//! `api.ts` listens for `chat-progress`. If the event name or the payload's
//! shape drifted, every other test would still pass and the panel would go
//! back to showing a finished wall of text with no sign anything had broken.

use super::*;
use std::sync::{Arc, Mutex};
use tauri::test::{mock_builder, mock_context, noop_assets};
use tauri::Listener;

/// An app on Tauri's mock runtime, and the events it has heard.
fn listening() -> (
    tauri::App<tauri::test::MockRuntime>,
    Arc<Mutex<Vec<String>>>,
) {
    let app = mock_builder()
        .build(mock_context(noop_assets()))
        .expect("the mock app builds");
    let heard: Arc<Mutex<Vec<String>>> = Arc::new(Mutex::new(Vec::new()));
    let sink = Arc::clone(&heard);
    app.listen("chat-progress", move |event| {
        sink.lock()
            .expect("the log")
            .push(event.payload().to_string());
    });
    (app, heard)
}

#[test]
fn the_growing_answer_reaches_the_window_as_chat_progress() {
    let (app, heard) = listening();
    let watcher = progress_events(app.handle().clone(), "draft");

    watcher("Take ");
    watcher("Take Bowers at 25.");

    let heard = heard.lock().expect("the log").clone();
    assert_eq!(heard.len(), 2, "one event per hand-over: {heard:?}");
    let payloads: Vec<serde_json::Value> = heard
        .iter()
        .map(|raw| serde_json::from_str(raw).expect("the payload is JSON"))
        .collect();
    // The shape `api.ts` listens for. A rename on either side and the panel
    // silently stops showing anything.
    assert_eq!(payloads[0]["screen"], "draft");
    assert_eq!(payloads[0]["text"], "Take ");
    // Each hand-over carries the whole answer so far, so the panel replaces
    // rather than appends and a dropped event costs nothing.
    assert_eq!(payloads[1]["text"], "Take Bowers at 25.");
    assert_eq!(
        payloads[1].as_object().map(|fields| fields.len()),
        Some(2),
        "two fields, and no third the panel would ignore: {:?}",
        payloads[1]
    );
}

/// Both screens' questions are answered through one command, so the screen has
/// to ride along: a panel showing the season must not paint the draft's answer
/// into itself.
#[test]
fn each_screens_answer_is_labelled_with_the_screen_that_asked() {
    let (app, heard) = listening();
    let draft = progress_events(app.handle().clone(), "draft");
    let season = progress_events(app.handle().clone(), "season");

    draft("who to take");
    season("who to start");

    let screens: Vec<String> = heard
        .lock()
        .expect("the log")
        .iter()
        .map(|raw| {
            serde_json::from_str::<serde_json::Value>(raw).expect("JSON")["screen"]
                .as_str()
                .unwrap_or_default()
                .to_string()
        })
        .collect();
    assert_eq!(screens, vec!["draft".to_string(), "season".to_string()]);
}

/// A closed window is not a reason to fail an answer that is otherwise fine:
/// the emit is allowed to go nowhere, and the watcher still returns.
#[test]
fn a_window_that_has_gone_does_not_take_the_answer_with_it() {
    let (app, heard) = listening();
    let watcher = progress_events(app.handle().clone(), "draft");
    drop(app);

    watcher("still writing");

    // Whether the bus still delivers after the app is dropped is Tauri's
    // business; what is asserted here is that the watcher does not panic and
    // the caller carries on.
    assert!(heard.lock().expect("the log").len() <= 1);
}
