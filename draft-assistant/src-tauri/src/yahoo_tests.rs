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
    assert_eq!(hosts.redirect_uri, OOB);
}
