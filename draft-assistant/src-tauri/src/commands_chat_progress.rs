//! The panel's window onto an answer that is still being written.
//!
//! The API sends an answer a few words at a time, and until now the backend
//! reassembled it in silence: the panel sat on "Thinking it through…" for the
//! length of the call and then painted a finished wall of text. `chat.rs`
//! hands the text so far to a watcher as it lands; this is the watcher the
//! desktop panel gets, which puts it on Tauri's event bus.

/// What the panel is told about an answer that is still arriving.
///
/// One event per hundred milliseconds at most (the throttle is in `chat.rs`),
/// carrying the whole of the text so far, so the panel replaces what it is
/// showing rather than appending to it and a missed event costs nothing.
#[derive(Clone, serde::Serialize)]
struct Progress {
    screen: String,
    text: String,
}

/// A watcher that puts the growing answer on the event bus.
///
/// The screen rides along because both screens' threads are answered through
/// one command, and a panel showing the season must not paint the draft's
/// answer into itself.
pub(crate) fn progress_events<R: tauri::Runtime>(
    app: tauri::AppHandle<R>,
    screen: &str,
) -> crate::chat::OnProgress {
    let screen = screen.to_string();
    std::sync::Arc::new(move |text: &str| {
        let progress = Progress {
            screen: screen.clone(),
            text: text.to_string(),
        };
        // A window that has gone is not an error worth failing an answer for.
        tauri::Emitter::emit(&app, "chat-progress", progress).ok();
    })
}

#[cfg(test)]
#[path = "commands_chat_progress_tests.rs"]
mod tests;
