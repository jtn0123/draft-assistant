//! The phone / second-screen companion: a small HTTP + WebSocket server the
//! host can turn on so a phone or a second copy of the app watches the same
//! league and shares one chat thread.
//!
//! Off by default, LAN only, and behind a six-digit pairing code. Nothing in
//! here ever serves a secret: `/api/config` is assembled field by field rather
//! than by serialising [`crate::engine::AppConfig`], which carries the API key.

pub mod hub;
pub mod media;
pub mod names;
pub mod net;
pub mod rand;
pub mod routes;
pub mod routes_chat;
pub mod server;
pub mod store;
pub mod ws;

pub use hub::{CompanionHub, Device};
pub use server::CompanionServer;

use std::sync::Arc;

/// Send an update to the paired devices, if this app is hosting any.
///
/// Called at each place that already emits the same update to the webview.
/// The hub is looked up rather than passed down so the poll loops keep their
/// existing shape, and so a build with no companion state managed — the
/// command tests run on one — is a no-op rather than a panic.
pub fn publish<R, T>(app: &tauri::AppHandle<R>, kind: &str, payload: &T)
where
    R: tauri::Runtime,
    T: serde::Serialize,
{
    use tauri::Manager;
    if let Some(companion) = app.try_state::<Arc<CompanionServer>>() {
        if companion.is_enabled() {
            companion.hub.publish(kind, payload);
        }
    }
}
