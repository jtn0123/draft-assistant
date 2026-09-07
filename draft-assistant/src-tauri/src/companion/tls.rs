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

/// A certificate ready to serve: the host it names, the rustls config, and
/// when it runs out (Unix seconds), which is what the periodic check reads
/// instead of the file.
pub struct Materials {
    pub host: String,
    pub config: Arc<rustls::ServerConfig>,
    pub not_after: Option<i64>,
}

/// What runs the `tailscale` arguments and says whether it succeeded.
/// Injected everywhere so no test ever spawns the CLI.
pub type Mint = Arc<dyn Fn(&[String]) -> bool + Send + Sync>;

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

/// What the files on disk say about the certificate.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum CertState {
    /// A file is missing, or the certificate cannot be read.
    Absent,
    /// Its `notAfter` has passed.
    Expired,
    /// Inside [`REMINT_WITHIN_SECS`] of running out: still good to serve,
    /// and time to ask for a new one.
    Renewable,
    Good,
}

/// The state of a certificate that runs out at `not_after`, at `now`.
pub fn state_at(not_after: Option<i64>, now: i64) -> CertState {
    match not_after {
        None => CertState::Absent,
        Some(end) if end <= now => CertState::Expired,
        Some(end) if end - now < REMINT_WITHIN_SECS => CertState::Renewable,
        Some(_) => CertState::Good,
    }
}

/// The state of the files on disk: the certificate PEM if it could be read,
/// whether the key is beside it, and `now` in Unix seconds.
pub fn cert_state(cert_pem: Option<&[u8]>, key_present: bool, now: i64) -> CertState {
    let Some(pem) = cert_pem else {
        return CertState::Absent;
    };
    if !key_present {
        return CertState::Absent;
    }
    state_at(tls_x509::not_after_from_pem(pem), now)
}

/// Whether the files on disk need `tailscale cert` run again: either is
/// missing, the certificate cannot be read, or it runs out within
/// [`REMINT_WITHIN_SECS`] of `now` (Unix seconds).
pub fn needs_mint(cert_pem: Option<&[u8]>, key_present: bool, now: i64) -> bool {
    cert_state(cert_pem, key_present, now) != CertState::Good
}

/// Find or mint the certificate for `name` under `dir`, as the two paths.
///
/// `run` is what executes the `tailscale` arguments and says whether it
/// succeeded; injected so the tests can write files where the CLI would and
/// never spawn it. The key file is made private however it got there:
/// Tailscale writes it 0600 already, and a copy made by hand may not be.
///
/// A renewal that fails is not the end of HTTPS: the certificate on disk is
/// still good for up to two weeks, so it is kept and the renewal is tried
/// again later. Only a certificate that is missing or has actually run out
/// turns a failed mint into an error.
pub fn ensure<R>(dir: &Path, name: &str, now: i64, run: R) -> Result<CertPaths, String>
where
    R: FnOnce(&[String]) -> bool,
{
    let paths = cert_paths(dir, name);
    let cert_pem = std::fs::read(&paths.cert).ok();
    let key_present = paths.key.is_file();
    let state = cert_state(cert_pem.as_deref(), key_present, now);
    if state != CertState::Good {
        std::fs::create_dir_all(dir)
            .map_err(|e| format!("could not make the certificate folder: {e}"))?;
        restrict(dir, 0o700);
        let minted = run(&mint_args(name, &paths)) && paths.cert.is_file() && paths.key.is_file();
        match (minted, state) {
            (true, _) => crate::applog::info(format!(
                "minted the phone connection's certificate for {name}"
            )),
            (false, CertState::Renewable) => crate::applog::warn(format!(
                "could not renew the phone connection's certificate for {name}; \
                 keeping the current one, which is good for a while yet, and trying again later"
            )),
            (false, _) => {
                return Err(format!(
                    "tailscale cert did not produce a certificate for {name}"
                ))
            }
        }
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
/// success and, on failure, a reason that never names the key. Every way it
/// fails is a warning: by the time this runs the machine has a MagicDNS name,
/// so a missing CLI or a refused mint is the reason the phone has no padlock.
pub fn run_tailscale(args: &[String]) -> bool {
    for path in CLI_PATHS {
        match std::process::Command::new(path).args(args).output() {
            Ok(output) if output.status.success() => return true,
            Ok(output) => {
                crate::applog::warn(format!(
                    "tailscale cert failed: {}",
                    String::from_utf8_lossy(&output.stderr).trim()
                ));
                return false;
            }
            Err(_) => continue,
        }
    }
    crate::applog::warn("no tailscale CLI found, so the phone connection stays http only");
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
    let not_after = std::fs::read(&paths.cert)
        .ok()
        .and_then(|pem| tls_x509::not_after_from_pem(&pem));
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
        not_after,
    })
}

/// The current time in Unix seconds.
pub fn now_secs() -> i64 {
    std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map(|d| d.as_secs() as i64)
        .unwrap_or(0)
}

/// What the listener should serve, if anything. `dns_name` is this machine's
/// MagicDNS name when Tailscale reported one. Spawns the CLI in the
/// `Tailscale` case, so it belongs on a blocking thread.
pub fn materials(source: &TlsSource, dns_name: Option<&str>) -> Option<Materials> {
    let name = match source {
        TlsSource::Off => return None,
        TlsSource::Files { host, .. } => host.as_str(),
        TlsSource::Tailscale { .. } => dns_name?,
    };
    materials_with(source, name, now_secs(), &run_tailscale)
}

/// The same over a name already chosen, a clock, and whatever stands in for
/// the CLI. `None` is the plain-HTTP companion of before. A machine with no
/// tailnet name never gets this far; a failure past that point is a warning
/// that says why the phone has no padlock, with nothing secret in it.
pub fn materials_with(
    source: &TlsSource,
    name: &str,
    now: i64,
    run: &(dyn Fn(&[String]) -> bool + Sync),
) -> Option<Materials> {
    let loaded = match source {
        TlsSource::Off => return None,
        TlsSource::Files { host, cert, key } => {
            let paths = CertPaths {
                cert: cert.clone(),
                key: key.clone(),
            };
            load(host, &paths)
        }
        TlsSource::Tailscale { dir } => {
            ensure(dir, name, now, run).and_then(|paths| load(name, &paths))
        }
    };
    match loaded {
        Ok(materials) => Some(materials),
        Err(why) => {
            crate::applog::warn(format!("phone connection stays http only: {why}"));
            None
        }
    }
}

#[cfg(test)]
#[path = "tls_tests.rs"]
pub(crate) mod tests;
