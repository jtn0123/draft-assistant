//! The certificate behind the companion's HTTPS listener.
//!
//! A phone page served over plain `http://` on a LAN address has no secure
//! context, and without one the browser refuses two things a draft needs:
//! the screen wake lock, and Add to Home Screen as a real app. Tailscale can
//! mint a certificate for this machine's MagicDNS name (`tailscale cert`),
//! signed by Let's Encrypt, so a phone on the tailnet gets a padlock with
//! nothing to install or trust by hand.
//!
//! Everything that decides is pure and tested against files in a temporary
//! directory; only [`run_tailscale`] spawns the CLI, and the tests never
//! reach it. The private key is never logged and is kept at mode 0600.

use super::net_tailscale::CLI_PATHS;
use super::tls_x509;
use std::path::{Path, PathBuf};
use std::sync::Arc;
use tokio_rustls::rustls;
use tokio_rustls::rustls::pki_types::{pem::PemObject, CertificateDer, PrivateKeyDer};

/// Where the listener's certificate comes from.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum TlsSource {
    /// No HTTPS listener. What the tests and the headless host run with, so
    /// no test ever asks the developer's Tailscale for a real certificate.
    Off,
    /// `tailscale cert` for this machine's MagicDNS name, kept under `dir`.
    Tailscale { dir: PathBuf },
    /// A certificate somebody else made for `host`, as PEM files. The tests
    /// mint one of these; nothing in the shipped app builds this variant.
    Files {
        host: String,
        cert: PathBuf,
        key: PathBuf,
    },
}

/// A certificate ready to serve: the host it names and the rustls config.
pub struct Materials {
    pub host: String,
    pub config: Arc<rustls::ServerConfig>,
}

/// The two files `tailscale cert` writes, for one name under one directory.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct CertPaths {
    pub cert: PathBuf,
    pub key: PathBuf,
}

/// A certificate this close to its `notAfter` is replaced. Let's Encrypt
/// issues for ninety days and Tailscale itself renews inside the last third;
/// two weeks leaves room for a laptop that is closed most of the month.
pub const REMINT_WITHIN_SECS: i64 = 14 * 86_400;

pub fn cert_paths(dir: &Path, name: &str) -> CertPaths {
    CertPaths {
        cert: dir.join(format!("{name}.crt")),
        key: dir.join(format!("{name}.key")),
    }
}

/// The arguments after `tailscale` that mint or renew the certificate.
pub fn mint_args(name: &str, paths: &CertPaths) -> Vec<String> {
    vec![
        "cert".to_string(),
        "--cert-file".to_string(),
        paths.cert.to_string_lossy().into_owned(),
        "--key-file".to_string(),
        paths.key.to_string_lossy().into_owned(),
        name.to_string(),
    ]
}

/// Whether the files on disk need `tailscale cert` run again: either is
/// missing, the certificate cannot be read, or it runs out within
/// [`REMINT_WITHIN_SECS`] of `now` (Unix seconds).
pub fn needs_mint(cert_pem: Option<&[u8]>, key_present: bool, now: i64) -> bool {
    let Some(pem) = cert_pem else {
        return true;
    };
    if !key_present {
        return true;
    }
    match tls_x509::not_after_from_pem(pem) {
        Some(not_after) => not_after - now < REMINT_WITHIN_SECS,
        None => true,
    }
}

/// Find or mint the certificate for `name` under `dir`, as the two paths.
///
/// `run` is what executes the `tailscale` arguments and says whether it
/// succeeded; injected so the tests can write files where the CLI would and
/// never spawn it. The key file is made private however it got there:
/// Tailscale writes it 0600 already, and a copy made by hand may not be.
pub fn ensure<R>(dir: &Path, name: &str, now: i64, run: R) -> Result<CertPaths, String>
where
    R: FnOnce(&[String]) -> bool,
{
    let paths = cert_paths(dir, name);
    let cert_pem = std::fs::read(&paths.cert).ok();
    let key_present = paths.key.is_file();
    if needs_mint(cert_pem.as_deref(), key_present, now) {
        std::fs::create_dir_all(dir)
            .map_err(|e| format!("could not make the certificate folder: {e}"))?;
        restrict(dir, 0o700);
        if !run(&mint_args(name, &paths)) {
            return Err(format!(
                "tailscale cert did not produce a certificate for {name}"
            ));
        }
        if !paths.cert.is_file() || !paths.key.is_file() {
            return Err(format!("tailscale cert wrote nothing for {name}"));
        }
        crate::applog::info(format!(
            "minted the phone connection's certificate for {name}"
        ));
    }
    restrict(&paths.key, 0o600);
    Ok(paths)
}

/// Unix permission bits on a file the app made. Nothing on other platforms;
/// nothing to do when the file is not there either.
fn restrict(path: &Path, mode: u32) {
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        let _ = std::fs::set_permissions(path, std::fs::Permissions::from_mode(mode));
    }
    #[cfg(not(unix))]
    {
        let _ = (path, mode);
    }
}

/// Run `tailscale <args>` from wherever the CLI is, as [`super::net_tailscale`]
/// finds it. Its output is discarded: `tailscale cert` prints nothing on
/// success and, on failure, a reason that never names the key.
fn run_tailscale(args: &[String]) -> bool {
    for path in CLI_PATHS {
        match std::process::Command::new(path).args(args).output() {
            Ok(output) if output.status.success() => return true,
            Ok(output) => {
                crate::applog::debug(format!(
                    "tailscale cert failed: {}",
                    String::from_utf8_lossy(&output.stderr).trim()
                ));
                return false;
            }
            Err(_) => continue,
        }
    }
    crate::applog::debug("no tailscale CLI, so the phone connection stays http only");
    false
}

/// The PEM pair as a rustls server config. The key bytes go straight into
/// rustls and are never formatted into a message.
pub fn load(host: &str, paths: &CertPaths) -> Result<Materials, String> {
    let certs: Vec<CertificateDer<'static>> = CertificateDer::pem_file_iter(&paths.cert)
        .map_err(|e| format!("could not read the certificate: {e}"))?
        .collect::<Result<_, _>>()
        .map_err(|e| format!("could not read the certificate: {e}"))?;
    if certs.is_empty() {
        return Err("the certificate file holds no certificate".to_string());
    }
    let key = PrivateKeyDer::from_pem_file(&paths.key)
        .map_err(|_| "could not read the certificate's key".to_string())?;
    // The provider is named rather than left to the default: rustls picks a
    // default only when exactly one is compiled in, and a future dependency
    // pulling the other in would turn every start into a panic.
    let config = rustls::ServerConfig::builder_with_provider(Arc::new(
        rustls::crypto::ring::default_provider(),
    ))
    .with_safe_default_protocol_versions()
    .map_err(|e| format!("could not set up TLS: {e}"))?
    .with_no_client_auth()
    .with_single_cert(certs, key)
    .map_err(|e| format!("the certificate and key do not go together: {e}"))?;
    Ok(Materials {
        host: host.to_string(),
        config: Arc::new(config),
    })
}

/// What the listener should serve, if anything. `dns_name` is this machine's
/// MagicDNS name when Tailscale reported one. Spawns the CLI in the
/// `Tailscale` case, so it belongs on a blocking thread.
///
/// `None` is the plain-HTTP companion of before, and every reason for it is
/// a debug line at most: a machine with no Tailscale is the common case and
/// not a fault.
pub fn materials(source: &TlsSource, dns_name: Option<&str>) -> Option<Materials> {
    match source {
        TlsSource::Off => None,
        TlsSource::Files { host, cert, key } => {
            let paths = CertPaths {
                cert: cert.clone(),
                key: key.clone(),
            };
            load(host, &paths).ok()
        }
        TlsSource::Tailscale { dir } => {
            let name = dns_name?;
            let now = std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .map(|d| d.as_secs() as i64)
                .unwrap_or(0);
            match ensure(dir, name, now, run_tailscale).and_then(|paths| load(name, &paths)) {
                Ok(materials) => Some(materials),
                Err(why) => {
                    crate::applog::debug(format!("phone connection stays http only: {why}"));
                    None
                }
            }
        }
    }
}

#[cfg(test)]
#[path = "tls_tests.rs"]
mod tests;
