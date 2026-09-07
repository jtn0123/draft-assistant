//! Accepting HTTPS connections and handing them to the same router the plain
//! listener serves.
//!
//! axum's own `serve` only speaks to a plain `TcpListener`, so the TLS side
//! is a small accept loop of its own: take a connection, finish the
//! handshake, and give hyper the router with the peer's address attached the
//! way `into_make_service_with_connect_info` would have, so the pairing
//! lockout still counts wrong codes per phone.
//!
//! The port is remembered between launches. A phone that installed the page
//! from `https://mac:7879/` has that address baked into its home-screen icon,
//! and a listener that came up on 7880 the next day stranded it.

use super::net::{self, Https};
use super::tls::Materials;
use axum::extract::ConnectInfo;
use axum::Router;
use hyper_util::rt::TokioIo;
use hyper_util::service::TowerToHyperService;
use std::net::SocketAddr;
use std::path::Path;
use std::sync::{Arc, RwLock};
use std::time::Duration;
use tokio::sync::oneshot;
use tokio_rustls::rustls::ServerConfig;
use tokio_rustls::TlsAcceptor;

/// The certificate the accept loop serves, swappable while it runs: a
/// renewal replaces what is inside without touching the port.
pub type LiveConfig = Arc<RwLock<Arc<ServerConfig>>>;

/// The HTTPS listener while it is up.
pub struct Listener {
    pub https: Https,
    /// When the certificate being served runs out (Unix seconds).
    pub not_after: Option<i64>,
    config: LiveConfig,
    /// Fired or dropped to bring the accept loop down.
    pub shutdown: oneshot::Sender<()>,
}

impl Listener {
    /// Serve a renewed certificate from now on, on the same port. Open
    /// connections finish on the old one; the next handshake gets the new.
    pub fn swap(&mut self, materials: Materials) {
        *self.config.write().unwrap_or_else(|e| e.into_inner()) = materials.config;
        self.not_after = materials.not_after;
    }
}

/// Bind `preferred` when given and still free, else the port after
/// `http_port` (or the next free one), and serve `router` over TLS there.
/// `None` when no port could be bound, which is a warning: the plain
/// listener is up, but the phone will not get its padlock.
pub fn start(
    http_port: u16,
    preferred: Option<u16>,
    materials: Materials,
    router: Router,
) -> Option<Listener> {
    let (listener, port) = match bind_preferring(preferred, http_port.saturating_add(1)) {
        Ok(bound) => bound,
        Err(why) => {
            crate::applog::warn(format!("phone connection stays http only: {why}"));
            return None;
        }
    };
    let listener = tokio::net::TcpListener::from_std(listener).ok()?;
    let (shutdown, stop) = oneshot::channel();
    let config: LiveConfig = Arc::new(RwLock::new(materials.config));
    spawn(listener, config.clone(), router, stop);
    Some(Listener {
        https: Https {
            host: materials.host,
            port,
        },
        not_after: materials.not_after,
        config,
        shutdown,
    })
}

/// The port from the last launch when it is still free, else the first free
/// one from `from`. The listener comes back non-blocking, as tokio needs it.
pub fn bind_preferring(
    preferred: Option<u16>,
    from: u16,
) -> Result<(std::net::TcpListener, u16), String> {
    if let Some(port) = preferred.filter(|p| *p != 0) {
        if let Ok(listener) = std::net::TcpListener::bind(("0.0.0.0", port)) {
            listener
                .set_nonblocking(true)
                .map_err(|e| format!("could not prepare the companion socket: {e}"))?;
            return Ok((listener, port));
        }
    }
    net::bind_from(from)
}

/// The HTTPS port the last launch took, from the file [`remember_port`]
/// wrote. `None` when there is no file or it does not hold a port.
pub fn remembered_port(file: &Path) -> Option<u16> {
    std::fs::read_to_string(file)
        .ok()?
        .trim()
        .parse::<u16>()
        .ok()
        .filter(|port| *port != 0)
}

/// Keep the HTTPS port for the next launch. A write that fails is not worth
/// failing the listener over: the next launch picks a port the old way.
pub fn remember_port(file: &Path, port: u16) {
    if let Some(dir) = file.parent() {
        let _ = std::fs::create_dir_all(dir);
    }
    let _ = std::fs::write(file, format!("{port}\n"));
}

/// The accept loop. Ends when `stop` fires or its sender is dropped, and
/// takes every connection it opened down with it: a phone's keep-alive
/// connection is otherwise nobody's to close once the listener is gone.
pub fn spawn(
    listener: tokio::net::TcpListener,
    config: LiveConfig,
    router: Router,
    mut stop: oneshot::Receiver<()>,
) -> tokio::task::JoinHandle<()> {
    tokio::spawn(async move {
        let mut connections: Vec<tokio::task::JoinHandle<()>> = Vec::new();
        loop {
            tokio::select! {
                _ = &mut stop => break,
                accepted = listener.accept() => {
                    let (stream, peer) = match accepted {
                        Ok(accepted) => accepted,
                        Err(_) => {
                            // Out of descriptors, or a connection that reset
                            // between accept and here. Neither is worth a
                            // tight loop; a short pause and try again.
                            tokio::time::sleep(Duration::from_millis(50)).await;
                            continue;
                        }
                    };
                    // The certificate as it is now, so a renewal that
                    // happened since the last handshake is what this one
                    // gets. An `Arc` clone per connection and nothing more.
                    let current = config.read().unwrap_or_else(|e| e.into_inner()).clone();
                    connections.retain(|task| !task.is_finished());
                    connections.push(tokio::spawn(serve_one(
                        stream,
                        peer,
                        TlsAcceptor::from(current),
                        router.clone(),
                    )));
                }
            }
        }
        for task in connections {
            task.abort();
        }
    })
}

/// One connection: the handshake, then HTTP/1.1 with upgrades, which is what
/// the event WebSocket needs. A handshake that fails is a port scanner or a
/// browser refusing the certificate, and neither is logged per connection.
async fn serve_one(
    stream: tokio::net::TcpStream,
    peer: SocketAddr,
    acceptor: TlsAcceptor,
    router: Router,
) {
    let Ok(tls) = acceptor.accept(stream).await else {
        return;
    };
    let service = TowerToHyperService::new(router.layer(axum::Extension(ConnectInfo(peer))));
    let _ = hyper::server::conn::http1::Builder::new()
        .serve_connection(TokioIo::new(tls), service)
        .with_upgrades()
        .await;
}
