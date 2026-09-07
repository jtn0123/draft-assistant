//! Keeping the HTTPS listener up for as long as the server is.
//!
//! HTTPS used to be decided once, when the server started: a Mac that joined
//! its tailnet ten minutes later showed an `http://` QR code until the toggle
//! was flipped off and on, and a certificate that ran out mid-season was
//! only noticed at the next launch. The origin refresh already looks at the
//! machine every thirty seconds; this is what it asks about the certificate
//! on each look. Every decision here is against an injected clock and an
//! injected stand-in for `tailscale cert`, so the tests never spawn the CLI.

use super::net::Https;
use super::net_tailscale::TailscaleSelf;
use super::tls::{self, CertState, Mint, TlsSource};
use super::tls_serve::{self, Listener};
use axum::Router;
use std::path::PathBuf;
use std::sync::Mutex;

/// How long after a mint or renewal that failed before it is tried again. A
/// refused `tailscale cert` every thirty seconds would fill the log with the
/// same warning and lean on Let's Encrypt for nothing.
pub const RETRY_AFTER_SECS: i64 = 60 * 60;

/// The HTTPS listener and what it takes to bring it up or keep it current.
pub struct Keeper {
    source: TlsSource,
    http_port: u16,
    /// Where the port taken is kept for the next launch.
    port_file: PathBuf,
    router: Router,
    mint: Mint,
    inner: Mutex<Inner>,
}

struct Inner {
    listener: Option<Listener>,
    /// No mint or renewal is attempted before this (Unix seconds).
    retry_after: i64,
}

impl Keeper {
    pub fn new(
        source: TlsSource,
        http_port: u16,
        port_file: PathBuf,
        router: Router,
        mint: Mint,
    ) -> Self {
        Self {
            source,
            http_port,
            port_file,
            router,
            mint,
            inner: Mutex::new(Inner {
                listener: None,
                retry_after: 0,
            }),
        }
    }

    fn lock(&self) -> std::sync::MutexGuard<'_, Inner> {
        self.inner.lock().unwrap_or_else(|e| e.into_inner())
    }

    /// The listener's address while it is up.
    pub fn https(&self) -> Option<Https> {
        self.lock().listener.as_ref().map(|l| l.https.clone())
    }

    /// One look at the machine: `this` is what Tailscale says about it now,
    /// `now` is Unix seconds. Brings HTTPS up when there is a name to serve
    /// for and a certificate to serve, renews the certificate inside its
    /// last two weeks, and takes HTTPS down only when the certificate has
    /// run out and could not be replaced. Spawns the CLI when a mint is due,
    /// so it belongs on a blocking thread. Hands back the address served.
    pub fn tick(&self, this: Option<&TailscaleSelf>, now: i64) -> Option<Https> {
        let name = match &self.source {
            TlsSource::Off => return None,
            TlsSource::Files { host, .. } => Some(host.clone()),
            TlsSource::Tailscale { .. } => this.and_then(|t| t.dns_name.clone()),
        };
        let mut inner = self.lock();
        let Some(name) = name else {
            // Off the tailnet, or Tailscale has not reported yet. A listener
            // already up keeps serving: its certificate is as good as it
            // was, and the phones will be back when the tailnet is.
            return inner.listener.as_ref().map(|l| l.https.clone());
        };
        match inner.listener.as_ref() {
            None => {
                if now < inner.retry_after {
                    return None;
                }
                inner.retry_after = now + RETRY_AFTER_SECS;
                let materials = tls::materials_with(&self.source, &name, now, &*self.mint)?;
                let preferred = tls_serve::remembered_port(&self.port_file);
                let listener =
                    tls_serve::start(self.http_port, preferred, materials, self.router.clone())?;
                tls_serve::remember_port(&self.port_file, listener.https.port);
                let https = listener.https.clone();
                inner.listener = Some(listener);
                Some(https)
            }
            Some(listener) => {
                let https = listener.https.clone();
                let state = tls::state_at(listener.not_after, now);
                if state == CertState::Good {
                    return Some(https);
                }
                if now >= inner.retry_after {
                    inner.retry_after = now + RETRY_AFTER_SECS;
                    if let Some(fresh) = tls::materials_with(&self.source, &name, now, &*self.mint)
                    {
                        let listener = inner.listener.as_mut().expect("checked above");
                        if fresh.not_after != listener.not_after {
                            crate::applog::info(format!(
                                "renewed the phone connection's certificate for {name}"
                            ));
                            listener.swap(fresh);
                        }
                        return Some(https);
                    }
                }
                if state == CertState::Expired {
                    crate::applog::warn(format!(
                        "the phone connection's certificate for {name} has run out and could not \
                         be renewed; phones are back to http only until it can be"
                    ));
                    if let Some(gone) = inner.listener.take() {
                        let _ = gone.shutdown.send(());
                    }
                    return None;
                }
                Some(https)
            }
        }
    }

    /// Bring the listener down, if it is up.
    pub fn stop(&self) {
        if let Some(listener) = self.lock().listener.take() {
            let _ = listener.shutdown.send(());
        }
    }
}

#[cfg(test)]
#[path = "tls_keeper_tests.rs"]
mod tests;
