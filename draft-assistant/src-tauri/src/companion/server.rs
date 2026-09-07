//! Starting and stopping the companion HTTP server, and the handle everything
//! else reaches it through.

use super::hub::{now_ms, CompanionHub, Emit};
use super::net;
use super::tls::{self, TlsSource};
use super::tls_keeper::Keeper;
use crate::shared_chat::SharedChat;
use crate::state::AppState;
use std::sync::{Arc, Mutex, OnceLock};
use std::time::Duration;
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
    /// Where the HTTPS listener's certificate comes from. The desktop asks
    /// Tailscale; the tests and the headless host run with it off.
    tls: Mutex<TlsSource>,
    /// Where the HTTPS port is kept between launches, so the address a phone
    /// installed the page from is the address it finds tomorrow.
    https_port_file: std::path::PathBuf,
}

struct Running {
    port: u16,
    /// Dropped or fired to bring the listener down.
    shutdown: oneshot::Sender<()>,
    /// The HTTPS listener beside it, and what brings it up when the tailnet
    /// appears later or renews its certificate.
    keeper: Arc<Keeper>,
    /// The code rotation and the origin refresh. Aborted on stop: both used
    /// to end on their own by noticing the port was gone, and a toggle off
    /// and on inside one tick left the old pair running beside the new one.
    tasks: Vec<tokio::task::JoinHandle<()>>,
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
            hub: Arc::new(CompanionHub::new(host_name, data_dir.clone())?),
            chat: Arc::new(SharedChat::new(data_dir.clone())),
            srv: OnceLock::new(),
            running: Mutex::new(None),
            tls: Mutex::new(TlsSource::Tailscale {
                dir: data_dir.join("companion-tls"),
            }),
            https_port_file: https_port_file(&data_dir),
        })
    }

    /// The same, but with the paired devices kept in a file inside the given
    /// data directory rather than in the machine's Keychain. This is what the
    /// tests build: a test run must never write a device token to the
    /// developer's real login Keychain.
    pub fn sandboxed(host_name: String, data_dir: std::path::PathBuf) -> Result<Self, String> {
        let secrets = Box::new(crate::yahoo_secrets::FileStore::in_dir(
            data_dir.join("secrets"),
        ));
        Ok(Self {
            hub: Arc::new(CompanionHub::with_secrets(
                host_name,
                data_dir.clone(),
                secrets,
            )?),
            chat: Arc::new(SharedChat::new(data_dir.clone())),
            srv: OnceLock::new(),
            running: Mutex::new(None),
            // Never the real Tailscale from a test: `tailscale cert` on the
            // developer's machine would mint a real certificate into a
            // scratch directory and count against Let's Encrypt's limits.
            tls: Mutex::new(TlsSource::Off),
            https_port_file: https_port_file(&data_dir),
        })
    }

    /// Where the next start looks for a certificate. Takes effect on the next
    /// `start`; a running listener keeps what it has.
    pub fn set_tls(&self, source: TlsSource) {
        *self.tls.lock().unwrap_or_else(|e| e.into_inner()) = source;
    }

    /// Where the listener's certificate comes from, as last set.
    pub fn tls_source(&self) -> TlsSource {
        self.tls.lock().unwrap_or_else(|e| e.into_inner()).clone()
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

    /// The port the HTTPS listener took, while there is one.
    pub fn https_port(&self) -> Option<u16> {
        let keeper = self.running().as_ref().map(|r| r.keeper.clone());
        keeper.and_then(|k| k.https()).map(|h| h.port)
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
        // Who this machine is on its tailnet, and the certificate for that
        // name if there is one to serve. Both shell out (the Tailscale CLI,
        // and `tailscale cert` on a first mint can take a few seconds), so
        // neither runs on the runtime's own threads. The keeper is asked
        // again on every origin refresh, so a tailnet joined after this, or
        // a certificate that runs out, is caught without a toggle.
        let keeper = Arc::new(Keeper::new(
            self.tls_source(),
            port,
            self.https_port_file.clone(),
            router.clone(),
            Arc::new(tls::run_tailscale),
        ));
        let looked = keeper.clone();
        let (this, secure) = tokio::task::spawn_blocking(move || {
            let this = net::tailscale_self();
            let secure = looked.tick(this.as_ref(), tls::now_secs());
            (this, secure)
        })
        .await
        .map_err(|e| format!("could not start the phone connection: {e}"))?;
        let (shutdown, wait) = oneshot::channel();
        tokio::spawn(async move {
            // With the peer's address attached to every request: the pairing
            // lockout counts wrong codes per address, so one machine guessing
            // cannot shut the rest of the house out.
            let service = router.into_make_service_with_connect_info::<std::net::SocketAddr>();
            let served = axum::serve(listener, service)
                .with_graceful_shutdown(async move {
                    // Either an explicit stop or the handle being dropped.
                    let _ = wait.await;
                })
                .await;
            if let Err(e) = served {
                crate::applog::warn(format!("the phone connection stopped: {e}"));
            }
        });
        self.hub.set_port(Some(port));
        // What the CSP, the cross-origin check and the tailnet URL on screen
        // are built from, read here and then on a slow timer: working out the
        // tailnet name shells out to `ifconfig` and the Tailscale CLI, and
        // doing that per request or per status read would put a process
        // spawn in front of every page load and every devices event.
        self.hub.set_reach(net::reach_from(
            port,
            &net::lan_ip(),
            this.as_ref(),
            secure.as_ref(),
        ));
        let looked = keeper.clone();
        let tasks = vec![
            spawn_rotation(self.hub.clone(), ROTATE_EVERY, now_ms),
            spawn_origin_refresh(self.hub.clone(), REFRESH_ORIGINS_EVERY, move |port| {
                let this = net::tailscale_self();
                let secure = looked.tick(this.as_ref(), tls::now_secs());
                net::reach_from(port, &net::lan_ip(), this.as_ref(), secure.as_ref())
            }),
        ];
        *self.running() = Some(Running {
            port,
            shutdown,
            keeper,
            tasks,
        });
        Ok(port)
    }

    /// Bring the server down. Paired devices are kept: turning the server off
    /// and on again is not the same gesture as Revoke, which is what throws
    /// devices off.
    pub fn stop(&self) {
        let running = self.running().take();
        // Off first, so nothing published between here and the last socket
        // closing reaches a phone; then every socket is told to go. The
        // graceful shutdown alone only stops the listener: an upgraded
        // WebSocket is detached from it, and the phones kept receiving
        // frames after the toggle was off.
        self.hub.set_port(None);
        self.hub.set_reach(net::Reach::default());
        self.hub.close_everyone();
        if let Some(running) = running {
            for task in running.tasks {
                task.abort();
            }
            let _ = running.shutdown.send(());
            running.keeper.stop();
        }
    }

    /// The URL to show, when there is one.
    pub fn url(&self) -> Option<String> {
        self.port().map(net::url_for)
    }

    /// The same server over Tailscale, when this machine is on a tailnet.
    ///
    /// This is the MagicDNS name where Tailscale can report one, so the QR
    /// code on screen survives the tailnet handing this node a new address.
    /// Read from the hub's cache, which the origin refresh keeps current;
    /// nothing is spawned to answer this.
    pub fn tailscale_url(&self) -> Option<String> {
        self.hub.tailscale_url()
    }

    /// The host opened another league. The phones hold the old league's
    /// chat and week until told otherwise, and nothing on the poll path says
    /// so: the draft loop publishes the new board, but the season is gone
    /// until its own screen is opened and the shared threads belong to the
    /// league now loaded. Both are sent here, as the socket's opening frames
    /// would send them.
    pub async fn league_switched(&self) {
        if !self.is_enabled() {
            return;
        }
        let Ok(srv) = self.srv() else {
            return;
        };
        self.hub
            .publish_json("season-updated", serde_json::Value::Null);
        let Ok(league_id) = super::routes_chat::active_league(&srv).await else {
            return;
        };
        for screen in ["draft", "season"] {
            let thread = srv.chat.thread(&league_id, screen).await;
            self.hub.publish("shared-chat", &thread);
        }
    }
}

/// Where the HTTPS port is remembered, under the app's data directory.
fn https_port_file(data_dir: &std::path::Path) -> std::path::PathBuf {
    data_dir.join("companion-tls").join("port")
}

/// How often an idle pairing code is looked at.
pub const ROTATE_EVERY: Duration = Duration::from_secs(60);

/// Keep the pairing code from sitting on the host's screen all afternoon.
///
/// [`CompanionHub::rotate_if_idle`] only ever ran when somebody asked for the
/// code, so a host whose Settings panel was closed showed the same six digits
/// until it was reopened. This is the thing that makes the ten minute life of
/// a code real. `clock` is the current time in milliseconds, injected so a
/// test can age a code without waiting ten minutes for one.
///
/// The task ends with the server: it stops as soon as the hub has no port.
pub fn spawn_rotation<C>(
    hub: Arc<CompanionHub>,
    every: Duration,
    clock: C,
) -> tokio::task::JoinHandle<()>
where
    C: Fn() -> u64 + Send + 'static,
{
    tokio::spawn(async move {
        let mut ticker = tokio::time::interval(every);
        // The first tick of a tokio interval is immediate, and a code made a
        // moment ago is not stale; skipping it keeps the log of what happened
        // when honest.
        ticker.tick().await;
        while hub.is_running() {
            ticker.tick().await;
            hub.rotate_if_idle(clock());
        }
    })
}

/// How often the machine's own addresses are looked at again.
pub const REFRESH_ORIGINS_EVERY: Duration = Duration::from_secs(30);

/// Keep the allowed origins, and the tailnet URL on screen, matching the
/// addresses the machine actually has.
///
/// They were read once when the server started, which was wrong the moment
/// Tailscale was installed or switched on afterwards: the phone could load
/// the page over the tailnet, and then the page's own `connect-src` refused
/// it the WebSocket until the server was turned off and on again. The same
/// went for a Wi-Fi change. `read` is what turns the port into the current
/// reading, injected so a test can move the machine without moving it.
///
/// Nothing is written when nothing changed, and the task ends with the
/// server, the same as the rotation. `read` shells out, and may run
/// `tailscale cert`, so it goes on a blocking thread each time.
pub fn spawn_origin_refresh<R>(
    hub: Arc<CompanionHub>,
    every: Duration,
    read: R,
) -> tokio::task::JoinHandle<()>
where
    R: Fn(u16) -> net::Reach + Send + Sync + 'static,
{
    let read = Arc::new(read);
    tokio::spawn(async move {
        let mut ticker = tokio::time::interval(every);
        ticker.tick().await;
        while let Some(port) = hub.port() {
            ticker.tick().await;
            // The server may have stopped, or been restarted on another port,
            // during the wait; only the port this loop was told about is ours
            // to write for.
            if hub.port() != Some(port) {
                break;
            }
            let look = read.clone();
            let Ok(fresh) = tokio::task::spawn_blocking(move || look(port)).await else {
                break;
            };
            let same = fresh.origins == hub.origins() && fresh.tailscale_url == hub.tailscale_url();
            if !same {
                crate::applog::info(format!(
                    "the phone connection's addresses changed: {}",
                    fresh.origins.join(" ")
                ));
                hub.set_reach(fresh);
            }
        }
    })
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

/// The phone page, compiled in. The files are written by the page lane;
/// serving them from the binary rather than from disk is what keeps the app a
/// single bundle with nothing to install beside it.
pub const INDEX_HTML: &str = include_str!("../../companion-static/index.html");
pub const HELPERS_JS: &str = include_str!("../../companion-static/helpers.js");
pub const CLOCK_JS: &str = include_str!("../../companion-static/clock.js");
pub const APP_JS: &str = include_str!("../../companion-static/app.js");
pub const APP_CSS: &str = include_str!("../../companion-static/app.css");
/// The installed-app half: the manifest and icon that make the page
/// installable, the service worker the browser requires for it (it caches
/// nothing), and the script that asks for the screen wake lock.
pub const PWA_JS: &str = include_str!("../../companion-static/pwa.js");
pub const SW_JS: &str = include_str!("../../companion-static/sw.js");
pub const MANIFEST: &str = include_str!("../../companion-static/manifest.webmanifest");
pub const ICON_SVG: &str = include_str!("../../companion-static/icon.svg");
/// The 180x180 PNG iOS wants for a home-screen icon; it ignores the SVG.
pub const TOUCH_ICON_PNG: &[u8] = include_bytes!("../../companion-static/apple-touch-icon.png");

/// The static file behind a `/static/{file}` path, with its content type.
///
/// An allow-list of names rather than a directory read: there is no path
/// to traverse, so no request can ask this for anything the page is not.
pub fn static_file(name: &str) -> Option<(&'static str, &'static [u8])> {
    match name {
        "index.html" => Some(("text/html; charset=utf-8", INDEX_HTML.as_bytes())),
        "helpers.js" => Some(("text/javascript; charset=utf-8", HELPERS_JS.as_bytes())),
        "clock.js" => Some(("text/javascript; charset=utf-8", CLOCK_JS.as_bytes())),
        "app.js" => Some(("text/javascript; charset=utf-8", APP_JS.as_bytes())),
        "app.css" => Some(("text/css; charset=utf-8", APP_CSS.as_bytes())),
        "pwa.js" => Some(("text/javascript; charset=utf-8", PWA_JS.as_bytes())),
        "sw.js" => Some(("text/javascript; charset=utf-8", SW_JS.as_bytes())),
        "manifest.webmanifest" => Some(("application/manifest+json", MANIFEST.as_bytes())),
        "icon.svg" => Some(("image/svg+xml", ICON_SVG.as_bytes())),
        "apple-touch-icon.png" => Some(("image/png", TOUCH_ICON_PNG)),
        _ => None,
    }
}

#[cfg(test)]
mod tests {
    use super::static_file;

    #[test]
    fn only_the_page_files_are_served() {
        for name in [
            "index.html",
            "helpers.js",
            "clock.js",
            "pwa.js",
            "app.js",
            "app.css",
            "sw.js",
            "manifest.webmanifest",
            "icon.svg",
            "apple-touch-icon.png",
        ] {
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
