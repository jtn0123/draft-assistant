//! The certificate decisions, against files in a temporary directory. No
//! test here runs `tailscale`; the closure that stands in for it writes a
//! self-signed pair minted with rcgen.

use super::{cert_paths, ensure, load, materials, mint_args, needs_mint, TlsSource};
use super::{CertPaths, REMINT_WITHIN_SECS};
use crate::companion::tls_x509::{days_from_civil, not_after_from_pem, not_after_unix};
use std::path::{Path, PathBuf};

pub const NAME: &str = "justins-mac.tail1234.ts.net";

fn scratch(label: &str) -> PathBuf {
    let dir = std::env::temp_dir().join(format!(
        "draft-assistant-tls-{label}-{}-{}",
        std::process::id(),
        std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .expect("clock")
            .as_nanos()
    ));
    std::fs::create_dir_all(&dir).expect("scratch dir");
    dir
}

/// A self-signed certificate for `name` that runs out at midnight on the
/// given day, as `(cert pem, key pem)`.
pub fn self_signed(name: &str, expires: (i32, u8, u8)) -> (String, String) {
    let mut params = rcgen::CertificateParams::new(vec![name.to_string()]).expect("params");
    params.not_before = rcgen::date_time_ymd(2026, 1, 1);
    params.not_after = rcgen::date_time_ymd(expires.0, expires.1, expires.2);
    let key = rcgen::KeyPair::generate().expect("a key pair");
    let cert = params.self_signed(&key).expect("a certificate");
    (cert.pem(), key.serialize_pem())
}

pub fn write_pair(paths: &CertPaths, pair: &(String, String)) {
    std::fs::write(&paths.cert, &pair.0).expect("cert written");
    std::fs::write(&paths.key, &pair.1).expect("key written");
}

fn midnight(year: i64, month: i64, day: i64) -> i64 {
    days_from_civil(year, month, day) * 86_400
}

#[test]
fn the_expiry_is_read_out_of_both_date_encodings() {
    // Certificates before 2050 carry a UTCTime, after it a GeneralizedTime;
    // Tailscale's are the first kind, and a parser that only knew one would
    // silently re-mint for ever on the other.
    let (soon, _) = self_signed(NAME, (2031, 3, 4));
    assert_eq!(
        not_after_from_pem(soon.as_bytes()),
        Some(midnight(2031, 3, 4))
    );
    let (later, _) = self_signed(NAME, (2060, 12, 31));
    assert_eq!(
        not_after_from_pem(later.as_bytes()),
        Some(midnight(2060, 12, 31))
    );
    assert_eq!(not_after_from_pem(b"not pem"), None);
    assert_eq!(not_after_unix(b"not der"), None);
}

#[test]
fn a_certificate_is_minted_when_missing_and_replaced_inside_two_weeks_of_expiry() {
    let now = midnight(2026, 9, 6);
    let (fresh, key) = self_signed(NAME, (2026, 12, 1));
    assert!(needs_mint(None, false, now), "no files at all");
    assert!(
        needs_mint(Some(fresh.as_bytes()), false, now),
        "cert but no key"
    );
    assert!(
        !needs_mint(Some(fresh.as_bytes()), true, now),
        "three months left"
    );
    let _ = key;
    // The failure this prevents: a certificate served until the day it ran
    // out, when every phone on the tailnet lost the padlock at once.
    let (soon, _) = self_signed(NAME, (2026, 9, 15));
    assert!(
        needs_mint(Some(soon.as_bytes()), true, now),
        "nine days left"
    );
    assert!(needs_mint(Some(b"garbage"), true, now), "unreadable");
    assert!(
        !needs_mint(
            Some(fresh.as_bytes()),
            true,
            midnight(2026, 12, 1) - REMINT_WITHIN_SECS - 1
        ),
        "one second outside the window is still good"
    );
}

#[test]
fn the_mint_command_names_the_files_under_the_app_folder_and_the_host() {
    let paths = cert_paths(Path::new("/tmp/app/companion-tls"), NAME);
    assert_eq!(
        paths.cert,
        PathBuf::from("/tmp/app/companion-tls/justins-mac.tail1234.ts.net.crt")
    );
    assert_eq!(
        paths.key,
        PathBuf::from("/tmp/app/companion-tls/justins-mac.tail1234.ts.net.key")
    );
    assert_eq!(
        mint_args(NAME, &paths),
        vec![
            "cert",
            "--cert-file",
            "/tmp/app/companion-tls/justins-mac.tail1234.ts.net.crt",
            "--key-file",
            "/tmp/app/companion-tls/justins-mac.tail1234.ts.net.key",
            NAME,
        ]
    );
}

#[test]
fn ensure_runs_the_cli_once_when_needed_and_leaves_the_key_private() {
    let dir = scratch("ensure");
    let now = midnight(2026, 9, 6);
    let pair = self_signed(NAME, (2026, 12, 1));
    let mut runs = 0;
    let paths = ensure(&dir, NAME, now, |args| {
        runs += 1;
        assert_eq!(args[0], "cert");
        write_pair(&cert_paths(&dir, NAME), &pair);
        true
    })
    .expect("minted");
    assert_eq!(runs, 1);
    assert!(paths.cert.is_file() && paths.key.is_file());
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        let mode = std::fs::metadata(&paths.key)
            .expect("key")
            .permissions()
            .mode()
            & 0o777;
        assert_eq!(mode, 0o600, "the key is readable by this user only");
    }
    // Good files on disk: the CLI is not run again.
    let again = ensure(&dir, NAME, now, |_| {
        panic!("tailscale run with a good cert on disk")
    })
    .expect("kept");
    assert_eq!(again, paths);
    // The CLI failing leaves an honest error, not a half-listener.
    let empty = scratch("ensure-fail");
    let failed = ensure(&empty, NAME, now, |_| false);
    assert!(failed.is_err());
    let _ = std::fs::remove_dir_all(dir);
    let _ = std::fs::remove_dir_all(empty);
}

#[test]
fn files_load_into_a_server_config_and_a_bad_pair_is_refused() {
    let dir = scratch("load");
    let paths = cert_paths(&dir, NAME);
    write_pair(&paths, &self_signed(NAME, (2027, 1, 1)));
    let loaded = load(NAME, &paths).expect("a config");
    assert_eq!(loaded.host, NAME);
    // A key that does not belong to the certificate.
    let (_, other_key) = self_signed(NAME, (2027, 1, 1));
    std::fs::write(&paths.key, other_key).expect("key swapped");
    let mismatch = load(NAME, &paths)
        .err()
        .expect("a mismatched pair is refused");
    assert!(!mismatch.contains("PRIVATE KEY"), "{mismatch}");
    let _ = std::fs::remove_dir_all(dir);
}

#[test]
fn off_and_no_magic_dns_name_mean_no_https_at_all() {
    // The failure this prevents: a machine with no Tailscale, or one whose
    // tailnet has no MagicDNS, trying to mint on every start.
    assert!(materials(&TlsSource::Off, Some(NAME)).is_none());
    let dir = scratch("nodns");
    assert!(materials(&TlsSource::Tailscale { dir: dir.clone() }, None).is_none());
    assert!(
        std::fs::read_dir(&dir).map(|d| d.count()).unwrap_or(0) == 0,
        "nothing was written without a name to mint for"
    );
    let missing = TlsSource::Files {
        host: NAME.to_string(),
        cert: dir.join("none.crt"),
        key: dir.join("none.key"),
    };
    assert!(materials(&missing, Some(NAME)).is_none());
    let _ = std::fs::remove_dir_all(dir);
}
