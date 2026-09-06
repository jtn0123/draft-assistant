//! The sign-in sessions that end somewhere other than "connected, paste a
//! code": a grant Yahoo revoked after the fact, and the loopback redirect the
//! app does not ship with but keeps live. Split from `yahoo_flows.rs` for the
//! line cap; the harness and the stubs are the same.

use crate::harness::{free_port, session, session_with_redirect, CLIENT_ID, CODE, SECRET};
use serde_json::json;
use std::sync::atomic::Ordering;

#[test]
fn a_grant_the_user_revoked_says_to_connect_again_and_settings_agrees() {
    // The failure this prevents: after the user revoked the app on Yahoo's
    // side, every call failed with "HTTP 401 for https://..." and the
    // Settings panel went on saying "Connected", with nothing anywhere
    // saying that signing in again was the fix.
    let s = session("yahoo-revoked");
    s.connect();
    s.ok("yahoo_leagues", json!({}));

    s.revoked.store(true, Ordering::SeqCst);
    let error = s.err("yahoo_leagues", json!({}));
    assert_eq!(error, "Yahoo signed you out. Connect again in Settings.");
    // The dead pair is gone, so the status says what the user has to do…
    let status = s.ok("yahoo_status", json!({}));
    assert_eq!(status["connected"], false);
    assert_eq!(
        status["configured"], true,
        "the registered app is not the grant"
    );
    // …and the next call does not go to Yahoo with it again.
    let error = s.err("yahoo_leagues", json!({}));
    assert!(error.contains("not connected to Yahoo"), "{error}");
    s.finish();
}

#[test]
fn a_loopback_redirect_finishes_the_sign_in_when_the_browser_comes_back() {
    // The app ships the paste-a-code flow; this is the other one, kept live
    // so that flipping `REDIRECT_FLOW` is a one-line change and not a
    // rewrite. Connect binds the port before the browser opens, the browser
    // (this test) comes back with the code, and the session is connected
    // without anything pasted.
    let port = free_port();
    let redirect = format!("http://localhost:{port}/");
    let s = session_with_redirect("yahoo-loopback", &redirect);
    s.ok(
        "yahoo_save_credentials",
        json!({"clientId": CLIENT_ID, "clientSecret": SECRET}),
    );
    assert_eq!(s.ok("yahoo_status", json!({}))["redirect"], redirect);
    let start = s.ok("yahoo_begin_connect", json!({}));
    let url = start["authorize_url"].as_str().expect("a URL to open");
    assert!(
        url.contains("redirect_uri=http%3A%2F%2Flocalhost%3A"),
        "{url}"
    );
    let state = start["state"].as_str().expect("a state").to_string();

    // The browser's one request to the listener, code and state on the query.
    let mut socket =
        std::net::TcpStream::connect(("127.0.0.1", port)).expect("the listener was bound");
    std::io::Write::write_all(
        &mut socket,
        format!("GET /?code={CODE}&state={state} HTTP/1.1\r\nHost: localhost\r\n\r\n").as_bytes(),
    )
    .expect("the browser's request");
    let mut page = String::new();
    let _ = std::io::Read::read_to_string(&mut socket, &mut page);
    assert!(page.contains("close this tab"), "{page}");

    let mut connected = false;
    for _ in 0..100 {
        if s.ok("yahoo_status", json!({}))["connected"] == true {
            connected = true;
            break;
        }
        std::thread::sleep(std::time::Duration::from_millis(50));
    }
    assert!(connected, "the loopback sign-in never finished");
    // The sign-in was consumed by the listener; there is nothing left to
    // paste a code into.
    let error = s.err(
        "yahoo_finish_connect",
        json!({"code": CODE, "state": state}),
    );
    assert!(error.contains("no Yahoo sign-in"), "{error}");
    s.finish();
}

#[test]
fn a_loopback_port_already_taken_fails_connect_before_the_browser_opens() {
    // Sending the user to Yahoo with nobody listening would leave them
    // approving an app that never hears back.
    let held = std::net::TcpListener::bind("127.0.0.1:0").expect("bind");
    let port = held.local_addr().expect("addr").port();
    let s = session_with_redirect("yahoo-loopback-taken", &format!("http://localhost:{port}/"));
    s.ok(
        "yahoo_save_credentials",
        json!({"clientId": CLIENT_ID, "clientSecret": SECRET}),
    );
    let error = s.err("yahoo_begin_connect", json!({}));
    assert!(error.contains(&format!("port {port}")), "{error}");
    drop(held);
    s.finish();
}
