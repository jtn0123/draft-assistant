//! Starting and stopping the companion HTTP server, and the handle everything
//! else reaches it through.

use super::hub::{CompanionHub, Emit};
use super::net;
use crate::shared_chat::SharedChat;
use crate::state::AppState;
use std::sync::{Arc, Mutex, OnceLock};
use tokio::sync::oneshot;

/// The companion as a whole: the state that outlives the socket, plus the
/// socket when it is up. One of these is managed by Tauri for the life of the
/// app; `enabled` is only ever about whether [`Running`] is present.
pub struct CompanionServer {
    pub hub: Arc<CompanionHub>,
    pub chat: Arc<SharedChat>,
    /// Filled in once, at startup, by [`CompanionServer::attach`]. The handlers
    /// and the desktop's own shared-chat commands share this one value, so a
    /// question asked from the phone and a question asked from the Mac run
    /// through the same objects.
    srv: OnceLock<Arc<Srv>>,
    running: Mutex<Option<Running>>,
}

struct Running {
    port: u16,
    /// Dropped or fired to bring the listener down.
    shutdown: oneshot::Sender<()>,
}

/// Everything a request handler is given.
pub struct Srv {
    pub hub: Arc<CompanionHub>,
    pub chat: Arc<SharedChat>,
    pub state: Arc<AppState>,
    pub emit: Emit,
}

impl CompanionServer {
    pub fn new(host_name: String, data_dir: std::path::PathBuf) -> Result<Self, String> {
        Ok(Self {
            hub: Arc::new(CompanionHub::new(host_name)?),
            chat: Arc::new(SharedChat::new(data_dir)),
            srv: OnceLock::new(),
            running: Mutex::new(None),
        })
    }

    /// Give the companion the app state and the way back to the webview.
    /// Called once, at startup; a second call is ignored rather than swapping
    /// the state out from under a running server.
    pub fn attach(self: &Arc<Self>, state: Arc<AppState>, emit: Emit) {
        self.hub.set_emit(emit.clone());
        let _ = self.srv.set(Arc::new(Srv {
            hub: self.hub.clone(),
            chat: self.chat.clone(),
            state,
            emit,
        }));
    }

    /// What the handlers and the desktop commands work through.
    pub fn srv(&self) -> Result<Arc<Srv>, String> {
        self.srv
            .get()
            .cloned()
            .ok_or_else(|| "the phone connection is not set up yet".to_string())
    }

    fn running(&self) -> std::sync::MutexGuard<'_, Option<Running>> {
        self.running.lock().unwrap_or_else(|e| e.into_inner())
    }

    pub fn port(&self) -> Option<u16> {
        self.running().as_ref().map(|r| r.port)
    }

    pub fn is_enabled(&self) -> bool {
        self.running().is_some()
    }

    /// Bring the server up on `first_port` or the next free one after it.
    /// Starting an already-running server is a no-op that reports the port it
    /// is on, so a second "Turn on" cannot leave two listeners behind.
    pub async fn start(&self, first_port: u16) -> Result<u16, String> {
        if let Some(port) = self.port() {
            return Ok(port);
        }
        let srv = self.srv()?;
        let (listener, port) = net::bind_from(first_port)?;
        let listener = tokio::net::TcpListener::from_std(listener)
            .map_err(|e| format!("could not start the phone connection: {e}"))?;
        let router = super::routes::router(srv);
        let (shutdown, wait) = oneshot::channel();
        tokio::spawn(async move {
            let served = axum::serve(listener, router)
                .with_graceful_shutdown(async move {
                    // Either an explicit stop or the handle being dropped.
                    let _ = wait.await;
                })
                .await;
            if let Err(e) = served {
                crate::applog::warn(format!("the phone connection stopped: {e}"));
            }
        });
        *self.running() = Some(Running { port, shutdown });
        self.hub.set_port(Some(port));
        Ok(port)
    }

    /// Bring the server down. Paired devices are kept: turning the server off
    /// and on again is not the same gesture as Revoke, which is what throws
    /// devices off.
    pub fn stop(&self) {
        if let Some(running) = self.running().take() {
            let _ = running.shutdown.send(());
        }
        self.hub.set_port(None);
    }

    /// The URL to show, when there is one.
    pub fn url(&self) -> Option<String> {
        self.port().map(net::url_for)
    }

    /// The same server over Tailscale, when this machine is on a tailnet.
    pub fn tailscale_url(&self) -> Option<String> {
        self.port().and_then(net::tailscale_url_for)
    }
}

impl Srv {
    /// Send a shared-chat thread everywhere it has to go: the paired devices
    /// over the WebSocket, and the host's own panel over the webview event.
    pub fn announce(&self, thread: &crate::shared_chat::SharedChatThread) {
        self.hub.publish("shared-chat", thread);
        match serde_json::to_value(thread) {
            Ok(value) => (self.emit)("shared-chat", value),
            Err(e) => crate::applog::warn(format!("could not send the shared chat on: {e}")),
        }
    }
}

/// The phone page, compiled in. The three files are written by the page lane;
/// serving them from the binary rather than from disk is what keeps the app a
/// single bundle with nothing to install beside it.
pub const INDEX_HTML: &str = include_str!("../../companion-static/index.html");
pub const HELPERS_JS: &str = include_str!("../../companion-static/helpers.js");
pub const APP_JS: &str = include_str!("../../companion-static/app.js");
pub const APP_CSS: &str = include_str!("../../companion-static/app.css");

/// The static file behind a `/static/{file}` path, with its content type.
///
/// An allow-list of three names rather than a directory read: there is no path
/// to traverse, so no request can ask this for anything the page is not.
pub fn static_file(name: &str) -> Option<(&'static str, &'static str)> {
    match name {
        "index.html" => Some(("text/html; charset=utf-8", INDEX_HTML)),
        "helpers.js" => Some(("text/javascript; charset=utf-8", HELPERS_JS)),
        "app.js" => Some(("text/javascript; charset=utf-8", APP_JS)),
        "app.css" => Some(("text/css; charset=utf-8", APP_CSS)),
        _ => None,
    }
}

#[cfg(test)]
mod tests {
    use super::static_file;

    #[test]
    fn only_the_page_files_are_served() {
        for name in ["index.html", "helpers.js", "app.js", "app.css"] {
            let (mime, body) = static_file(name).expect("{name} is served");
            assert!(!mime.is_empty());
            assert!(!body.is_empty(), "{name} is empty");
        }
        // No directory read behind this, so nothing to traverse out of.
        assert!(static_file("../../src/engine.rs").is_none());
        assert!(static_file("config.json").is_none());
        assert!(static_file("").is_none());
    }
}
