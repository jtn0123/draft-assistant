use super::*;

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
    assert_eq!(stub.matching("get_token").len(), 1, "one refresh, not two");
    // The failure this prevents: a revoked grant read "HTTP 401 for <url>"
    // and Settings went on saying "Connected". The error names the fix, and
    // the client says the pair is dead so whoever persists it clears it.
    assert_eq!(error, YahooError::SignedOut, "{error:?}");
    assert_eq!(error.to_string(), SIGNED_OUT);
    assert!(
        client.signed_out(),
        "the client did not mark the grant gone"
    );
}

#[tokio::test]
async fn a_refresh_that_yahoo_refuses_signs_the_user_out_without_the_secret() {
    let stub = serve(move |request: &Request| {
        if request.path() == "/oauth2/get_token" {
            Reply::status(
                400,
                format!(r#"{{"error":"invalid_grant","description":"{SECRET} is wrong"}}"#),
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
    // A 400 from the token endpoint is Yahoo's word that the refresh token
    // is dead; the user has to sign in again and is told so in those words.
    assert_eq!(error, YahooError::SignedOut, "{error:?}");
    assert_eq!(error.to_string(), SIGNED_OUT);
    assert!(!error.to_string().contains(SECRET), "the secret escaped");
    assert!(client.signed_out());
    assert_eq!(
        stub.matching("/teams").len(),
        0,
        "a call went out with no token"
    );
}

#[tokio::test]
async fn a_token_endpoint_outage_is_not_a_sign_out() {
    // Yahoo being down for a minute must not cost the user the pair: a 5xx
    // stays an auth failure the next call can retry, and nothing is cleared.
    let stub = serve(move |request: &Request| {
        if request.path() == "/oauth2/get_token" {
            Reply::status(503, r#"{"error":"try later"}"#)
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
    assert!(error.to_string().contains("503"), "{error}");
    assert!(
        !client.signed_out(),
        "an outage was mistaken for a revoked grant"
    );
    assert_eq!(client.tokens().await.refresh_token, "refresh-1");
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
    let in_flight = Hits::new();
    let stub = serve({
        let in_flight = in_flight.clone();
        move |request: &Request| {
            if request.path() == "/oauth2/get_token" {
                in_flight.bump("get_token");
                std::thread::sleep(std::time::Duration::from_millis(600));
                Reply::ok(FRESH_TOKEN)
            } else {
                Reply::ok(TEAMS)
            }
        }
    });
    let client = std::sync::Arc::new(client_for(&stub, stale_tokens()));
    let loading = tokio::spawn({
        let client = client.clone();
        async move { client.league_teams(LEAGUE_KEY).await }
    });
    // Until the refresh is in flight: the router signals as it starts its slow
    // answer (the stub only records a request once the router has returned),
    // so the token read below is made mid-refresh rather than after a fixed
    // pause that may or may not have got there.
    let started = std::time::Instant::now();
    while in_flight.seen("get_token") == 0 {
        assert!(
            started.elapsed() < std::time::Duration::from_secs(10),
            "the refresh never reached the stub"
        );
        tokio::time::sleep(std::time::Duration::from_millis(5)).await;
    }
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
