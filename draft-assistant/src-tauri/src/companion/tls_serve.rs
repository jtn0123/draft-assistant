//! Accepting HTTPS connections and handing them to the same router the plain
//! listener serves.
//!
//! axum's own `serve` only speaks to a plain `TcpListener`, so the TLS side
//! is a small accept loop of its own: take a connection, finish the
//! handshake, and give hyper the router with the peer's address attached the
//! way `into_make_service_with_connect_info` would have, so the pairing
//! lockout still counts wrong codes per phone.

use super::net::{self, Https};
use super::tls::Materials;
use axum::extract::ConnectInfo;
use axum::Router;
use hyper_util::rt::TokioIo;
use hyper_util::service::TowerToHyperService;
use std::net::SocketAddr;
use std::time::Duration;
use tokio::sync::oneshot;
use tokio_rustls::TlsAcceptor;

/// The HTTPS listener while it is up.
pub struct Listener {
    pub https: Https,
    /// Fired or dropped to bring the accept loop down.
    pub shutdown: oneshot::Sender<()>,
}

/// Bind the port after `http_port` (or the next free one) and serve `router`
/// over TLS there. `None` when no port could be bound, which is logged at
/// debug: the plain listener is up and that is the companion of before.
pub fn start(http_port: u16, materials: Materials, router: Router) -> Option<Listener> {
    let (listener, port) = match net::bind_from(http_port.saturating_add(1)) {
        Ok(bound) => bound,
        Err(why) => {
            crate::applog::debug(format!("phone connection stays http only: {why}"));
            return None;
        }
    };
    let listener = tokio::net::TcpListener::from_std(listener).ok()?;
    let (shutdown, stop) = oneshot::channel();
    spawn(listener, materials.config, router, stop);
    Some(Listener {
        https: Https {
            host: materials.host,
            port,
        },
        shutdown,
    })
}

/// The accept loop. Ends when `stop` fires or its sender is dropped, and
/// takes every connection it opened down with it: a phone's keep-alive
/// connection is otherwise nobody's to close once the listener is gone.
pub fn spawn(
    listener: tokio::net::TcpListener,
    config: std::sync::Arc<tokio_rustls::rustls::ServerConfig>,
    router: Router,
    mut stop: oneshot::Receiver<()>,
) -> tokio::task::JoinHandle<()> {
    let acceptor = TlsAcceptor::from(config);
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
                    connections.retain(|task| !task.is_finished());
                    connections.push(tokio::spawn(serve_one(
                        stream,
                        peer,
                        acceptor.clone(),
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
