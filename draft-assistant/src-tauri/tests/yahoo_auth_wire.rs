//! The token half of the Yahoo wire: the code exchange, the proactive
//! refresh, the 401-then-refresh-then-retry, and what happens when Yahoo says
//! no. `tests/yahoo_wire.rs` covers the resources; the stub they share is
//! `tests/yahoo_stub/mod.rs`.

mod yahoo_stub;

use draft_assistant_lib::yahoo::{YahooClient, YahooError, YahooHosts};
use draft_assistant_lib::yahoo_oauth::{AuthError, OauthClient, TokenSet, YahooCredentials, OOB};
use yahoo_stub::{serve, Hits, Reply, Request, Stub};

const TEAMS: &str = include_str!("fixtures/yahoo/teams.json");
const LEAGUE_KEY: &str = "449.l.12345";
const SECRET: &str = "top-secret-client-secret";
/// base64("dj0yJmk9wireclient:top-secret-client-secret")
const BASIC: &str = "Basic ZGoweUptazl3aXJlY2xpZW50OnRvcC1zZWNyZXQtY2xpZW50LXNlY3JldA==";
const FRESH_TOKEN: &str =
    r#"{"access_token":"access-2","refresh_token":"refresh-2","expires_in":3600}"#;

fn credentials() -> YahooCredentials {
    YahooCredentials {
        client_id: "dj0yJmk9wireclient".into(),
        client_secret: SECRET.into(),
    }
}

/// A token pair that is good for another hour.
fn live_tokens() -> TokenSet {
    TokenSet {
        access_token: "access-1".into(),
        refresh_token: "refresh-1".into(),
        expires_at: draft_assistant_lib::yahoo_oauth::now_secs() + 3_600,
    }
}

/// A token pair that expired an hour ago.
fn stale_tokens() -> TokenSet {
    TokenSet {
        access_token: "access-stale".into(),
        refresh_token: "refresh-1".into(),
        expires_at: draft_assistant_lib::yahoo_oauth::now_secs().saturating_sub(3_600),
    }
}

fn hosts(stub: &Stub) -> YahooHosts {
    YahooHosts {
        api_base: format!("{}/fantasy/v2", stub.base()),
        login_base: stub.base(),
        redirect_uri: "oob".into(),
    }
}

fn client_for(stub: &Stub, tokens: TokenSet) -> YahooClient {
    YahooClient::with_hosts(credentials(), tokens, hosts(stub))
}

#[tokio::test]
async fn a_code_is_exchanged_for_a_token_pair() {
    let stub = serve(|_: &Request| Reply::ok(FRESH_TOKEN));
    let client = OauthClient::with_base(stub.base());
    let tokens = client
        .exchange_code(&credentials(), "  auth-code-1  ", OOB)
        .await
        .expect("the code is good");
    assert_eq!(tokens.access_token, "access-2");
    assert_eq!(tokens.refresh_token, "refresh-2");
    assert!(!tokens.is_expired(draft_assistant_lib::yahoo_oauth::now_secs()));

    let request = stub.requests().pop().expect("one request");
    assert_eq!(request.method, "POST");
    assert_eq!(request.path(), "/oauth2/get_token");
    assert_eq!(request.header("authorization"), Some(BASIC));
    assert_eq!(
        request.form("grant_type").as_deref(),
        Some("authorization_code")
    );
    // Trimmed, and sent in the body rather than the URL.
    assert_eq!(request.form("code").as_deref(), Some("auth-code-1"));
    assert_eq!(request.form("redirect_uri").as_deref(), Some("oob"));
    assert!(
        request.target.split('?').nth(1).is_none(),
        "{}",
        request.target
    );
}

#[tokio::test]
async fn a_loopback_exchange_repeats_the_redirect_uri_yahoo_registered() {
    let stub = serve(|_: &Request| Reply::ok(FRESH_TOKEN));
    let client = OauthClient::with_base(stub.base());
    client
        .exchange_code(&credentials(), "code", "http://localhost:8731/")
        .await
        .expect("the code is good");
    assert_eq!(
        stub.requests()[0].form("redirect_uri").as_deref(),
        Some("http://localhost:8731/")
    );
}

#[tokio::test]
async fn a_rejected_code_reports_yahoos_status_without_the_secret() {
    let stub = serve(|_: &Request| {
        Reply::status(
            400,
            format!(r#"{{"error":"invalid_grant","description":"{SECRET} and a bad code"}}"#),
        )
    });
    let client = OauthClient::with_base(stub.base());
    let error = client
        .exchange_code(&credentials(), "stale-code", OOB)
        .await
        .expect_err("that code was used already");
    assert!(
        matches!(error, AuthError::Http { status: 400, .. }),
        "{error:?}"
    );
    let message = error.to_string();
    assert!(message.contains("invalid_grant"), "{message}");
    assert!(!message.contains(SECRET), "the secret escaped: {message}");
}

#[tokio::test]
async fn a_token_reply_that_is_not_a_token_is_a_decode_failure() {
    let stub = serve(|_: &Request| Reply::ok("<html>maintenance</html>"));
    let client = OauthClient::with_base(stub.base());
    let error = client
        .exchange_code(&credentials(), "code", OOB)
        .await
        .expect_err("that was not a token");
    assert!(matches!(error, AuthError::Decode(_)), "{error:?}");
}

#[tokio::test]
async fn a_login_host_that_is_not_there_is_a_transport_failure() {
    let client = OauthClient::with_base("http://127.0.0.1:1");
    let error = client
        .exchange_code(&credentials(), "code", OOB)
        .await
        .expect_err("port 1 answers nobody");
    assert!(matches!(error, AuthError::Transport(_)), "{error:?}");
    assert!(!error.to_string().contains(SECRET));
}

#[tokio::test]
async fn a_401_is_answered_by_refreshing_the_token_and_asking_again() {
    let hits = Hits::new();
    let counter = hits.clone();
    let stub = serve(move |request: &Request| {
        if request.path() == "/oauth2/get_token" {
            return Reply::ok(FRESH_TOKEN);
        }
        match counter.bump("api") {
            1 => Reply::status(401, r#"{"error":"token_expired"}"#),
            _ => Reply::ok(TEAMS),
        }
    });
    let client = client_for(&stub, live_tokens());
    let teams = client
        .league_teams(LEAGUE_KEY)
        .await
        .expect("the retry succeeds");
    assert_eq!(teams.len(), 3);

    let requests = stub.requests();
    assert_eq!(requests.len(), 3, "call, refresh, call again");
    assert_eq!(requests[0].header("authorization"), Some("Bearer access-1"));
    assert_eq!(requests[1].path(), "/oauth2/get_token");
    assert_eq!(requests[2].header("authorization"), Some("Bearer access-2"));
    // And the renewed pair is what the caller would persist.
    let stored = client.tokens().await;
    assert_eq!(stored.access_token, "access-2");
    assert_eq!(stored.refresh_token, "refresh-2");
}

#[tokio::test]
async fn the_refresh_request_is_the_documented_one() {
    let stub = serve(move |request: &Request| {
        if request.path() == "/oauth2/get_token" {
            Reply::ok(FRESH_TOKEN)
        } else {
            Reply::ok(TEAMS)
        }
    });
    // Stale tokens, so the refresh happens before the first call goes out.
    let client = client_for(&stub, stale_tokens());
    client.league_teams(LEAGUE_KEY).await.expect("teams load");

    let refresh = stub
        .matching("get_token")
        .pop()
        .expect("a refresh was sent");
    assert_eq!(refresh.method, "POST");
    assert_eq!(
        refresh.header("content-type"),
        Some("application/x-www-form-urlencoded")
    );
    // base64("dj0yJmk9wireclient:top-secret-client-secret")
    assert_eq!(
        refresh.header("authorization"),
        Some("Basic ZGoweUptazl3aXJlY2xpZW50OnRvcC1zZWNyZXQtY2xpZW50LXNlY3JldA==")
    );
    assert_eq!(refresh.form("grant_type").as_deref(), Some("refresh_token"));
    assert_eq!(refresh.form("refresh_token").as_deref(), Some("refresh-1"));
    assert_eq!(refresh.form("redirect_uri").as_deref(), Some("oob"));
    // The secret rides in the header, never in the body.
    assert!(!refresh.body.contains(SECRET), "{}", refresh.body);
}

#[tokio::test]
async fn an_expired_token_is_renewed_before_the_call_rather_than_after_a_401() {
    let stub = serve(move |request: &Request| {
        if request.path() == "/oauth2/get_token" {
            Reply::ok(FRESH_TOKEN)
        } else {
            Reply::ok(TEAMS)
        }
    });
    let client = client_for(&stub, stale_tokens());
    client.league_teams(LEAGUE_KEY).await.expect("teams load");
    let requests = stub.requests();
    assert_eq!(requests.len(), 2, "refresh, then the one call");
    assert_eq!(requests[0].path(), "/oauth2/get_token");
    assert_eq!(requests[1].header("authorization"), Some("Bearer access-2"));
}

#[tokio::test]
async fn a_second_401_gives_up_rather_than_spending_the_refresh_token_again() {
    let stub = serve(move |request: &Request| {
        if request.path() == "/oauth2/get_token" {
            Reply::ok(FRESH_TOKEN)
        } else {
            Reply::status(401, r#"{"error":"invalid_token"}"#)
        }
    });
    let client = client_for(&stub, live_tokens());
    let error = client
        .league_teams(LEAGUE_KEY)
        .await
        .expect_err("the grant is gone");
    assert!(
        matches!(error, YahooError::Http { status: 401, .. }),
        "{error:?}"
    );
    assert_eq!(stub.matching("get_token").len(), 1, "one refresh, not two");
}

#[tokio::test]
async fn a_refresh_that_yahoo_refuses_reports_an_auth_failure_without_the_secret() {
    let stub = serve(move |request: &Request| {
        if request.path() == "/oauth2/get_token" {
            Reply::status(
                400,
                format!(r#"{{"error":"invalid_client","description":"{SECRET} is wrong"}}"#),
            )
        } else {
            Reply::ok(TEAMS)
        }
    });
    let client = client_for(&stub, stale_tokens());
    let error = client
        .league_teams(LEAGUE_KEY)
        .await
        .expect_err("no token, no call");
    assert!(matches!(error, YahooError::Auth(_)), "{error:?}");
    let message = error.to_string();
    assert!(message.contains("400"), "{message}");
    assert!(!message.contains(SECRET), "the secret escaped: {message}");
}

#[tokio::test]
async fn several_calls_that_find_the_token_expired_refresh_it_once_between_them() {
    // The failure this prevents: a board load fires its Yahoo reads together,
    // and when the access token had just run out every one of them spent the
    // refresh token in turn. Yahoo rotates that token on each use, so the
    // second refresh raced the first and could sign the user out mid-draft.
    let stub = serve(move |request: &Request| {
        if request.path() == "/oauth2/get_token" {
            // Slow enough that the other two callers are certainly waiting.
            std::thread::sleep(std::time::Duration::from_millis(150));
            Reply::ok(FRESH_TOKEN)
        } else {
            Reply::ok(TEAMS)
        }
    });
    let client = std::sync::Arc::new(client_for(&stub, stale_tokens()));
    let calls: Vec<_> = (0..3)
        .map(|_| {
            let client = client.clone();
            tokio::spawn(async move { client.league_teams(LEAGUE_KEY).await })
        })
        .collect();
    for call in calls {
        call.await
            .expect("the task finished")
            .expect("the teams load");
    }
    assert_eq!(
        stub.matching("get_token").len(),
        1,
        "the refresh token was spent more than once"
    );
    // And the two that waited used what the first one brought back rather
    // than the token they found expired.
    for request in stub
        .requests()
        .iter()
        .filter(|r| r.path().ends_with("/teams"))
    {
        assert_eq!(request.header("authorization"), Some("Bearer access-2"));
    }
    assert_eq!(client.tokens().await.access_token, "access-2");
}

#[tokio::test]
async fn a_refresh_in_flight_does_not_freeze_everything_else_holding_the_client() {
    // The failure this prevents: the token pair's lock used to be held across
    // the refresh request, so a Yahoo that took ten seconds to answer froze
    // every other caller — the poller included — for those ten seconds.
    let stub = serve(move |request: &Request| {
        if request.path() == "/oauth2/get_token" {
            std::thread::sleep(std::time::Duration::from_millis(600));
            Reply::ok(FRESH_TOKEN)
        } else {
            Reply::ok(TEAMS)
        }
    });
    let client = std::sync::Arc::new(client_for(&stub, stale_tokens()));
    let loading = tokio::spawn({
        let client = client.clone();
        async move { client.league_teams(LEAGUE_KEY).await }
    });
    tokio::time::sleep(std::time::Duration::from_millis(100)).await;
    let asked_at = std::time::Instant::now();
    let held = client.tokens().await;
    assert!(
        asked_at.elapsed() < std::time::Duration::from_millis(200),
        "reading the tokens waited on the refresh: {:?}",
        asked_at.elapsed()
    );
    // Mid-refresh the pair is still the old one, which is exactly what a
    // caller that only wants to persist it should see.
    assert_eq!(held.access_token, "access-stale");
    loading
        .await
        .expect("the task finished")
        .expect("the teams load");
    assert_eq!(client.tokens().await.access_token, "access-2");
}
