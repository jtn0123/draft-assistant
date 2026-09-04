//! The pure halves of the OAuth flow: the URLs, the header, the form, the
//! expiry arithmetic, the redirect parser, and the loopback listener driven
//! over a real socket. Nothing here talks to Yahoo.

use super::*;
// `TcpListener` and the io traits used to arrive with `super::*`; the loopback
// listener has its own module now, so the socket-driving tests name them.
use std::io::{Read, Write};
use std::net::TcpListener;

fn creds() -> YahooCredentials {
    YahooCredentials {
        client_id: "dj0yJmk9testclientid".into(),
        client_secret: "0123456789abcdefsecret".into(),
    }
}

#[test]
fn the_authorize_url_names_yahoos_own_endpoint() {
    let url = authorize_url("id", OOB, "st");
    assert!(
        url.starts_with("https://api.login.yahoo.com/oauth2/request_auth?"),
        "{url}"
    );
    assert!(url.contains("response_type=code"), "{url}");
    assert!(url.contains("client_id=id"), "{url}");
    assert!(url.contains("redirect_uri=oob"), "{url}");
    assert!(url.contains("state=st"), "{url}");
}

#[test]
fn a_loopback_redirect_uri_is_escaped_into_the_authorize_url() {
    let url = authorize_url("id", "http://localhost:8731/", "st");
    assert!(
        url.contains("redirect_uri=http%3A%2F%2Flocalhost%3A8731%2F"),
        "{url}"
    );
}

#[test]
fn the_authorize_url_never_carries_the_secret() {
    // The secret belongs in a header on a POST, never in a URL the browser --
    // and every proxy and history file between here and Yahoo -- can see.
    let url = authorize_url_on(LOGIN_BASE, &creds().client_id, OOB, "st");
    assert!(!url.contains(&creds().client_secret), "{url}");
}

#[test]
fn the_basic_header_is_the_documented_base64_of_id_and_secret() {
    let header = basic_header(&YahooCredentials {
        client_id: "abc".into(),
        client_secret: "123".into(),
    });
    // base64("abc:123")
    assert_eq!(header, "Basic YWJjOjEyMw==");
}

#[test]
fn base64_matches_the_rfc_4648_vectors() {
    for (input, expected) in [
        ("", ""),
        ("f", "Zg=="),
        ("fo", "Zm8="),
        ("foo", "Zm9v"),
        ("foob", "Zm9vYg=="),
        ("fooba", "Zm9vYmE="),
        ("foobar", "Zm9vYmFy"),
    ] {
        assert_eq!(base64(input.as_bytes()), expected, "for {input:?}");
    }
}

#[test]
fn base64_encodes_bytes_outside_ascii() {
    assert_eq!(base64(&[0xff, 0xee, 0xdd]), "/+7d");
}

#[test]
fn the_code_exchange_body_is_the_documented_one() {
    let form = token_form(Grant::Code("abc123"), OOB);
    assert!(form.contains(&("grant_type".into(), "authorization_code".into())));
    assert!(form.contains(&("code".into(), "abc123".into())));
    assert!(form.contains(&("redirect_uri".into(), "oob".into())));
    assert!(form.iter().all(|(name, _)| name != "client_secret"));
}

#[test]
fn the_refresh_body_sends_the_refresh_token_and_no_code() {
    let form = token_form(Grant::Refresh("r-1"), "http://localhost:8731/");
    assert!(form.contains(&("grant_type".into(), "refresh_token".into())));
    assert!(form.contains(&("refresh_token".into(), "r-1".into())));
    assert!(form.iter().all(|(name, _)| name != "code"));
    assert!(form.contains(&("redirect_uri".into(), "http://localhost:8731/".to_string())));
}

#[test]
fn a_token_is_treated_as_expired_a_minute_before_it_actually_is() {
    let tokens = TokenSet {
        access_token: "a".into(),
        refresh_token: "r".into(),
        expires_at: 1_000,
    };
    assert!(!tokens.is_expired(1_000 - SKEW - 1));
    // Inside the skew window: still valid on Yahoo's clock, refreshed anyway.
    assert!(tokens.is_expired(1_000 - SKEW));
    assert!(tokens.is_expired(1_000));
    assert!(tokens.is_expired(2_000));
}

#[test]
fn an_expiry_far_in_the_future_never_looks_expired_at_zero() {
    let tokens = TokenSet {
        access_token: "a".into(),
        refresh_token: "r".into(),
        expires_at: u64::MAX,
    };
    assert!(!tokens.is_expired(u64::MAX - SKEW - 1));
}

#[test]
fn the_expiry_is_now_plus_what_yahoo_said() {
    let parsed: TokenResponse =
        serde_json::from_str(r#"{"access_token":"a","refresh_token":"r","expires_in":3600}"#)
            .expect("parse");
    let tokens = token_set(parsed, None, 1_000);
    assert_eq!(tokens.expires_at, 4_600);
    assert_eq!(tokens.refresh_token, "r");
}

#[test]
fn a_refresh_that_returns_no_new_refresh_token_keeps_the_old_one() {
    let parsed: TokenResponse =
        serde_json::from_str(r#"{"access_token":"a2","expires_in":3600}"#).expect("parse");
    let tokens = token_set(parsed, Some("r-old"), 10);
    assert_eq!(tokens.refresh_token, "r-old");
    assert_eq!(tokens.access_token, "a2");
}

#[test]
fn a_reply_without_a_lifetime_is_treated_as_already_stale() {
    let parsed: TokenResponse = serde_json::from_str(r#"{"access_token":"a"}"#).expect("parse");
    let tokens = token_set(parsed, Some("r"), 500);
    assert_eq!(tokens.expires_at, 500);
    assert!(tokens.is_expired(500));
}

#[test]
fn redaction_removes_the_secret_from_anything_on_its_way_out() {
    let secret = "0123456789abcdefsecret";
    let leaked = format!("invalid_client: {secret} was rejected");
    let clean = redact(&leaked, secret);
    assert!(!clean.contains(secret), "{clean}");
    assert!(clean.contains("***"), "{clean}");
}

#[test]
fn redaction_leaves_a_message_alone_when_there_is_no_secret_to_remove() {
    assert_eq!(redact("plain", ""), "plain");
}

#[test]
fn an_error_body_is_flattened_and_capped() {
    let detail = trim_detail(&format!("line one\n  line two{}", "x".repeat(400)));
    assert!(!detail.contains('\n'));
    assert!(detail.ends_with("..."));
    assert!(detail.chars().count() <= 203);
}

#[test]
fn errors_read_as_sentences() {
    assert_eq!(
        AuthError::Http {
            status: 400,
            detail: "invalid_grant".into()
        }
        .to_string(),
        "Yahoo login returned HTTP 400: invalid_grant"
    );
    assert_eq!(
        AuthError::Invalid("no authorization code was given".into()).to_string(),
        "no authorization code was given"
    );
}

#[tokio::test]
async fn an_empty_code_never_reaches_the_network() {
    // Port 1 answers nothing; reaching it at all would fail differently.
    let client = OauthClient::with_base("http://127.0.0.1:1");
    let error = client
        .exchange_code(&creds(), "   ", OOB)
        .await
        .expect_err("an empty code cannot be exchanged");
    assert!(matches!(error, AuthError::Invalid(_)), "{error:?}");
}

#[tokio::test]
async fn an_empty_refresh_token_never_reaches_the_network() {
    let client = OauthClient::with_base("http://127.0.0.1:1");
    let error = client
        .refresh(&creds(), "", OOB)
        .await
        .expect_err("there is nothing to refresh with");
    assert!(matches!(error, AuthError::Invalid(_)), "{error:?}");
}

#[test]
fn the_redirect_query_gives_up_the_code_and_the_state() {
    let redirect = parse_redirect("/?code=abc123&state=xyz");
    assert_eq!(redirect.code, "abc123");
    assert_eq!(redirect.state, "xyz");
}

#[test]
fn a_percent_escaped_redirect_is_decoded() {
    let redirect = parse_redirect("/?state=a%2Fb+c&code=x%3Dy");
    assert_eq!(redirect.state, "a/b c");
    assert_eq!(redirect.code, "x=y");
}

#[test]
fn a_redirect_yahoo_refused_carries_no_code() {
    let redirect = parse_redirect("/?error=access_denied&state=xyz");
    assert!(redirect.code.is_empty());
    assert_eq!(redirect.state, "xyz");
}

#[test]
fn a_bare_path_is_not_a_redirect() {
    assert_eq!(parse_redirect("/favicon.ico"), Redirect::default());
}

/// Drive the loopback listener the way a browser would, over a real socket.
fn browser_get(target: &str) -> (Result<Redirect, AuthError>, String) {
    let listener = TcpListener::bind("127.0.0.1:0").expect("bind");
    let port = listener.local_addr().expect("addr").port();
    let caught = std::thread::spawn(move || catch_redirect_on(listener));
    let mut socket = std::net::TcpStream::connect(("127.0.0.1", port)).expect("connect");
    socket
        .write_all(format!("GET {target} HTTP/1.1\r\nHost: localhost\r\n\r\n").as_bytes())
        .expect("write");
    let mut page = String::new();
    let _ = socket.read_to_string(&mut page);
    (caught.join().expect("listener thread"), page)
}

#[test]
fn the_listener_takes_the_code_and_tells_the_user_to_close_the_tab() {
    let (caught, page) = browser_get("/?code=live-code&state=nonce-1");
    let redirect = caught.expect("the redirect carried a code");
    assert_eq!(redirect.code, "live-code");
    assert_eq!(redirect.state, "nonce-1");
    assert!(page.starts_with("HTTP/1.1 200 OK"), "{page}");
    assert!(page.contains("close this tab"), "{page}");
}

#[test]
fn a_redirect_without_a_code_still_answers_the_browser_but_fails_the_flow() {
    let (caught, page) = browser_get("/?error=access_denied");
    assert!(page.starts_with("HTTP/1.1 200 OK"), "{page}");
    assert!(page.contains("No authorization code"), "{page}");
    let error = caught.expect_err("no code means no flow");
    assert!(matches!(error, AuthError::Invalid(_)), "{error:?}");
}

#[test]
fn a_port_already_in_use_is_reported_rather_than_panicked() {
    let held = TcpListener::bind("127.0.0.1:0").expect("bind");
    let port = held.local_addr().expect("addr").port();
    let error = catch_redirect(port).expect_err("the port is taken");
    assert!(matches!(error, AuthError::Invalid(_)), "{error:?}");
}

#[test]
fn the_authorize_url_asks_for_the_read_only_fantasy_scope() {
    let url = authorize_url("id", OOB, "st");
    assert!(url.contains("scope=fspt-r"), "{url}");
    // Read, not write: nothing this app is issued can change a league.
    assert!(!url.contains("fspt-w"), "{url}");
}

#[test]
fn a_debug_print_of_the_credentials_never_carries_either_half() {
    let printed = format!("{:?}", creds());
    assert!(!printed.contains(&creds().client_secret), "{printed}");
    assert!(!printed.contains(&creds().client_id), "{printed}");
    assert!(printed.contains("YahooCredentials"), "{printed}");
    // And nested inside something else, which is how a `{:?}` usually happens.
    let nested = format!("{:?}", Some(vec![creds()]));
    assert!(!nested.contains(&creds().client_secret), "{nested}");
}

#[test]
fn a_debug_print_of_a_token_set_keeps_the_expiry_and_nothing_else() {
    let tokens = TokenSet {
        access_token: "at-supersecret".into(),
        refresh_token: "rt-supersecret".into(),
        expires_at: 1_800_000_000,
    };
    let printed = format!("{tokens:?}");
    assert!(!printed.contains("supersecret"), "{printed}");
    assert!(printed.contains("1800000000"), "{printed}");
}

#[test]
fn a_browser_that_never_comes_back_gives_up_instead_of_waiting_forever() {
    let listener = TcpListener::bind("127.0.0.1:0").expect("bind");
    let started = std::time::Instant::now();
    let error = catch_redirect_on_within(listener, std::time::Duration::from_millis(150))
        .expect_err("nothing connected, so there is no redirect");
    assert!(
        matches!(error, AuthError::Transport(_)),
        "{error:?} should read as the browser not arriving"
    );
    assert!(error.to_string().contains("never came back"), "{error}");
    // The point of the change: it returned rather than blocking on `accept`.
    assert!(
        started.elapsed() < std::time::Duration::from_secs(5),
        "the wait was not bounded: {:?}",
        started.elapsed()
    );
}

#[test]
fn the_app_itself_waits_long_enough_for_a_real_sign_in() {
    // Five minutes: a Yahoo login with two-factor on the end of it, and no
    // more — an abandoned connect must not hold the port for the session.
    assert_eq!(REDIRECT_WAIT, std::time::Duration::from_secs(300));
}
