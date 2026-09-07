//! The keeper against files in a temporary directory and a made-up tailnet.
//! The stand-in for `tailscale cert` either writes a self-signed pair where
//! the CLI would, or refuses; the CLI itself is never run.

use super::Keeper;
use crate::applog::Capture;
use crate::companion::net_tailscale::TailscaleSelf;
use crate::companion::tls::tests::{self_signed, write_pair, NAME};
use crate::companion::tls::{cert_paths, TlsSource};
use crate::companion::tls_x509::days_from_civil;
use std::path::{Path, PathBuf};
use std::sync::Arc;

fn scratch(label: &str) -> PathBuf {
    let dir = std::env::temp_dir().join(format!(
        "draft-assistant-keeper-{label}-{}-{}",
        std::process::id(),
        std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .expect("clock")
            .as_nanos()
    ));
    std::fs::create_dir_all(&dir).expect("scratch dir");
    dir
}

fn midnight(year: i64, month: i64, day: i64) -> i64 {
    days_from_civil(year, month, day) * 86_400
}

fn on_tailnet() -> TailscaleSelf {
    TailscaleSelf {
        dns_name: Some(NAME.to_string()),
        ip: Some("100.101.102.103".to_string()),
    }
}

/// A keeper over the Tailscale source under `dir`, with the CLI stood in
/// for by `mint`. Port 0 for the plain listener, so the fallback is a free
/// port the kernel picks rather than 1.
fn keeper(dir: &Path, mint: impl Fn(&[String]) -> bool + Send + Sync + 'static) -> Keeper {
    Keeper::new(
        TlsSource::Tailscale {
            dir: dir.to_path_buf(),
        },
        0,
        dir.join("port"),
        axum::Router::new(),
        Arc::new(mint),
    )
}

fn answers(port: u16) -> bool {
    std::net::TcpStream::connect(("127.0.0.1", port)).is_ok()
}

/// The failure this prevents: HTTPS was decided once at start, so a Mac
/// that joined its tailnet afterwards showed an http QR until toggled.
#[tokio::test]
async fn https_comes_up_when_the_tailnet_appears_after_the_server_started() {
    let dir = scratch("later");
    write_pair(&cert_paths(&dir, NAME), &self_signed(NAME, (2027, 1, 1)));
    let keeper = keeper(&dir, |_| panic!("good files on disk, nothing to mint"));
    let now = midnight(2026, 9, 6);
    assert_eq!(
        keeper.tick(None, now),
        None,
        "no name yet, nothing to serve for"
    );
    assert!(
        std::fs::read_dir(&dir).expect("dir").count() == 2,
        "nothing else written"
    );
    let https = keeper
        .tick(Some(&on_tailnet()), now + 30)
        .expect("the tailnet appeared");
    assert_eq!(https.host, NAME);
    assert!(answers(https.port));
    // And it stays up through a later look, and while off the tailnet.
    assert_eq!(
        keeper.tick(Some(&on_tailnet()), now + 60),
        Some(https.clone())
    );
    assert_eq!(keeper.tick(None, now + 90), Some(https.clone()));
    keeper.stop();
    tokio::time::sleep(std::time::Duration::from_millis(50)).await;
    assert!(!answers(https.port));
    let _ = std::fs::remove_dir_all(dir);
}

/// The failure this prevents: the listener took the next free port after
/// the plain one, so a page installed from `https://mac:7879/` was stranded
/// the day 7879 happened to be busy.
#[tokio::test]
async fn the_https_port_is_the_same_on_the_next_launch_and_moves_only_when_taken() {
    let dir = scratch("port");
    write_pair(&cert_paths(&dir, NAME), &self_signed(NAME, (2027, 1, 1)));
    let now = midnight(2026, 9, 6);
    let first = keeper(&dir, |_| false);
    let port = first.tick(Some(&on_tailnet()), now).expect("up").port;
    first.stop();
    tokio::time::sleep(std::time::Duration::from_millis(50)).await;
    let second = keeper(&dir, |_| false);
    assert_eq!(
        second
            .tick(Some(&on_tailnet()), now)
            .expect("up again")
            .port,
        port,
        "the phone's home-screen icon still points here"
    );
    // A third, while the second still holds the port, has to go elsewhere.
    let third = keeper(&dir, |_| false);
    let moved = third
        .tick(Some(&on_tailnet()), now)
        .expect("up beside it")
        .port;
    assert_ne!(moved, port);
    second.stop();
    third.stop();
    let _ = std::fs::remove_dir_all(dir);
}

/// Inside the last two weeks the certificate is replaced in place: the
/// port does not change, and the next handshake gets the new one.
#[tokio::test]
async fn a_certificate_in_its_last_two_weeks_is_renewed_on_the_same_port() {
    let dir = scratch("renew");
    write_pair(&cert_paths(&dir, NAME), &self_signed(NAME, (2026, 9, 15)));
    let paths = cert_paths(&dir, NAME);
    let fresh = self_signed(NAME, (2026, 12, 15));
    let minted = Arc::new(std::sync::atomic::AtomicUsize::new(0));
    let count = minted.clone();
    let keeper = keeper(&dir, move |_| {
        count.fetch_add(1, std::sync::atomic::Ordering::SeqCst);
        write_pair(&paths, &fresh);
        true
    });
    let now = midnight(2026, 9, 6);
    // Nine days left: the first look mints straight away.
    let capture = Capture::start();
    let https = keeper.tick(Some(&on_tailnet()), now).expect("up");
    assert_eq!(minted.load(std::sync::atomic::Ordering::SeqCst), 1);
    assert!(capture.saw("INFO minted the phone connection's certificate"));
    // Now good for three months: later looks cost nothing.
    assert_eq!(
        keeper.tick(Some(&on_tailnet()), now + 86_400),
        Some(https.clone())
    );
    assert_eq!(minted.load(std::sync::atomic::Ordering::SeqCst), 1);
    keeper.stop();
    let _ = std::fs::remove_dir_all(dir);
}

#[tokio::test]
async fn a_renewal_that_fails_keeps_https_up_and_is_retried_an_hour_later_not_sooner() {
    let dir = scratch("renew-fail");
    write_pair(&cert_paths(&dir, NAME), &self_signed(NAME, (2027, 1, 10)));
    let tried = Arc::new(std::sync::atomic::AtomicUsize::new(0));
    let count = tried.clone();
    let keeper = keeper(&dir, move |_| {
        count.fetch_add(1, std::sync::atomic::Ordering::SeqCst);
        false
    });
    let good = midnight(2026, 9, 6);
    let https = keeper.tick(Some(&on_tailnet()), good).expect("up");
    assert_eq!(tried.load(std::sync::atomic::Ordering::SeqCst), 0);
    // Into the window: a renewal is tried and refused, and HTTPS stays.
    let capture = Capture::start();
    let window = midnight(2027, 1, 1);
    assert_eq!(
        keeper.tick(Some(&on_tailnet()), window),
        Some(https.clone())
    );
    assert_eq!(tried.load(std::sync::atomic::Ordering::SeqCst), 1);
    assert!(capture.saw("WARN could not renew"), "{:?}", capture.lines());
    assert!(answers(https.port));
    // Thirty seconds on: not tried again.
    assert_eq!(
        keeper.tick(Some(&on_tailnet()), window + 30),
        Some(https.clone())
    );
    assert_eq!(tried.load(std::sync::atomic::Ordering::SeqCst), 1);
    // An hour on: tried again.
    assert_eq!(
        keeper.tick(Some(&on_tailnet()), window + super::RETRY_AFTER_SECS),
        Some(https.clone())
    );
    assert_eq!(tried.load(std::sync::atomic::Ordering::SeqCst), 2);
    keeper.stop();
    let _ = std::fs::remove_dir_all(dir);
}

/// Only a certificate that has actually run out takes HTTPS down, and the
/// log says so at warn.
#[tokio::test]
async fn an_expired_certificate_that_cannot_be_replaced_drops_https_with_a_warning() {
    let dir = scratch("expired");
    write_pair(&cert_paths(&dir, NAME), &self_signed(NAME, (2026, 9, 15)));
    let keeper = keeper(&dir, |_| false);
    // Brought up nine days out, over a renewal that fails: still served.
    let https = keeper
        .tick(Some(&on_tailnet()), midnight(2026, 9, 6))
        .expect("kept on the nine-day certificate");
    let capture = Capture::start();
    // Retry gate still shut, so the look at the expiry alone drops it.
    assert_eq!(
        keeper.tick(Some(&on_tailnet()), midnight(2026, 9, 15) + 30),
        None
    );
    assert!(
        capture.saw("WARN the phone connection's certificate for") && capture.saw("has run out"),
        "{:?}",
        capture.lines()
    );
    tokio::time::sleep(std::time::Duration::from_millis(50)).await;
    assert!(
        !answers(https.port),
        "the listener is gone with the certificate"
    );
    assert_eq!(keeper.https(), None);
    let _ = std::fs::remove_dir_all(dir);
}

#[test]
fn the_remembered_port_is_read_back_and_nonsense_is_ignored() {
    use crate::companion::tls_serve::{remember_port, remembered_port};
    let dir = scratch("port-file");
    let file = dir.join("nested").join("port");
    assert_eq!(remembered_port(&file), None);
    remember_port(&file, 7879);
    assert_eq!(remembered_port(&file), Some(7879));
    std::fs::write(&file, "not a port").expect("written");
    assert_eq!(remembered_port(&file), None);
    std::fs::write(&file, "0").expect("written");
    assert_eq!(remembered_port(&file), None);
    let _ = std::fs::remove_dir_all(dir);
}
