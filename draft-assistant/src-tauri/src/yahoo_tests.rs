use super::*;

#[test]
fn every_path_asks_for_json() {
    assert_eq!(
        url_for(BASE, "/league/449.l.1/teams"),
        "https://fantasysports.yahooapis.com/fantasy/v2/league/449.l.1/teams?format=json"
    );
}

#[test]
fn a_path_that_already_has_a_query_gets_an_ampersand() {
    assert_eq!(
        url_for("http://127.0.0.1:1/v2", "/league/1/players?x=1"),
        "http://127.0.0.1:1/v2/league/1/players?x=1&format=json"
    );
}

#[test]
fn matrix_parameters_are_not_mistaken_for_a_query() {
    // Yahoo separates sub-resource parameters with `;`, so the first `?`
    // is still ours to add.
    let url = url_for(BASE, "/league/449.l.1/players;start=0;count=25");
    assert!(
        url.ends_with("players;start=0;count=25?format=json"),
        "{url}"
    );
}

#[test]
fn keys_that_could_escape_the_path_are_refused() {
    for bad in ["", "449.l.1/../../users", "449.l.1;out=x", "a b"] {
        assert!(
            check_key("league", bad).is_err(),
            "{bad:?} should not be a legal key"
        );
    }
    assert!(check_key("league", "449.l.12345.t.7").is_ok());
}

#[test]
fn a_throttled_caller_is_told_to_wait_rather_than_shown_yahoos_own_status() {
    for status in RATE_LIMITED {
        let error = YahooError::Http {
            status,
            url: "https://fantasysports.yahooapis.com/x".into(),
        };
        assert!(error.retryable(), "{status} should be worth repeating");
        let said = error.to_string();
        assert_eq!(
            said,
            "Yahoo is rate-limiting requests — try again in a minute"
        );
        assert!(!said.contains(&status.to_string()), "{said}");
    }
}

#[test]
fn only_transport_and_server_errors_are_worth_repeating() {
    assert!(YahooError::Transport {
        url: "u".into(),
        detail: "reset".into()
    }
    .retryable());
    assert!(YahooError::Http {
        status: 503,
        url: "u".into()
    }
    .retryable());
    for status in [400, 401, 404] {
        assert!(!YahooError::Http {
            status,
            url: "u".into()
        }
        .retryable());
    }
    assert!(!YahooError::Invalid("no".into()).retryable());
}

#[test]
fn the_default_hosts_are_yahoos_own() {
    let hosts = YahooHosts::default();
    assert_eq!(hosts.api_base, BASE);
    assert_eq!(hosts.login_base, LOGIN_BASE);
    // The redirect is whichever flow the app ships with, spelled in one
    // place; the tokens Yahoo issues are bound to it, so it cannot drift from
    // what the Connect command sent the browser off with.
    assert_eq!(hosts.redirect_uri, redirect_uri());
}

#[test]
fn a_revoked_grant_reads_as_sign_in_again_rather_than_as_an_http_status() {
    // The failure this prevents: a revoked grant surfaced as "HTTP 401 for
    // https://..." and nothing told the user the one thing that fixes it.
    let error = YahooError::SignedOut;
    assert_eq!(
        error.to_string(),
        "Yahoo signed you out. Connect again in Settings."
    );
    assert!(
        !error.retryable(),
        "repeating a call against a dead grant only spends the refresh token"
    );
    assert!(GRANT_GONE.contains(&400) && GRANT_GONE.contains(&401));
    assert!(
        !GRANT_GONE.contains(&503),
        "a Yahoo outage is not a sign-out"
    );
}

fn creds() -> YahooCredentials {
    YahooCredentials {
        client_id: "dj0yJmk9unit".into(),
        client_secret: "unit-secret".into(),
    }
}

/// Hosts nobody answers on: port 1 refuses every connection, so a request
/// that does go out fails in a way these tests would notice.
fn dead_hosts() -> YahooHosts {
    YahooHosts {
        api_base: "http://127.0.0.1:1/fantasy/v2".into(),
        login_base: "http://127.0.0.1:1".into(),
        redirect_uri: "oob".into(),
    }
}

#[tokio::test]
async fn a_stored_pair_with_no_refresh_token_is_a_sign_out_not_a_permanent_error() {
    // The failure this prevents: a pair whose refresh token was empty came
    // back as "no refresh token is stored", an auth error that is neither
    // retryable nor a sign-out. The pair stayed in the Keychain, Settings
    // said "Connected", and every call failed the same way until the user
    // guessed that Disconnect was the fix.
    let stale = TokenSet {
        access_token: "access-stale".into(),
        refresh_token: String::new(),
        expires_at: 0,
    };
    let client = YahooClient::with_hosts(creds(), stale, dead_hosts());
    let error = client
        .league_teams("449.l.1")
        .await
        .expect_err("nothing to renew with");
    assert_eq!(error, YahooError::SignedOut, "{error:?}");
    assert!(
        client.signed_out(),
        "whoever persists the pair has to be told to clear it"
    );
}

#[tokio::test]
async fn the_pair_counts_as_unsaved_only_once_a_refresh_has_changed_it() {
    let first = TokenSet {
        access_token: "access-1".into(),
        refresh_token: "refresh-1".into(),
        expires_at: u64::MAX,
    };
    let client = YahooClient::with_hosts(creds(), first.clone(), dead_hosts());
    // Built from what the store holds: nothing to write.
    assert!(client.unsaved_tokens().await.is_none());
    let renewed = TokenSet {
        access_token: "access-2".into(),
        ..first.clone()
    };
    *client.tokens.lock().await = renewed.clone();
    assert_eq!(client.unsaved_tokens().await, Some(renewed.clone()));
    // Still unsaved until the caller says the write landed.
    assert_eq!(client.unsaved_tokens().await, Some(renewed.clone()));
    client.mark_persisted(&renewed).await;
    assert!(client.unsaved_tokens().await.is_none());
    // A stale acknowledgement does not cover a newer pair.
    *client.tokens.lock().await = first.clone();
    client.mark_persisted(&renewed).await;
    assert_eq!(client.unsaved_tokens().await, Some(first));
}
