//! The token half of the Yahoo wire: the code exchange, the proactive
//! refresh, the 401-then-refresh-then-retry, and what happens when Yahoo says
//! no. `tests/yahoo_wire.rs` covers the resources; the stub they share is
//! `tests/yahoo_stub/mod.rs`.
//!
//! One test binary in three parts: the helpers every part shares live here,
//! and each part is `use super::*` away from them.

#[path = "../yahoo_stub/mod.rs"]
mod yahoo_stub;

/// The code exchange, and what Yahoo's refusals turn into.
mod exchange;
/// Writing the pair back: once per refresh, and cleared when the grant is gone.
mod persist;
/// The refresh, proactive and after a 401, and the sign-out it can end in.
mod refresh;

use draft_assistant_lib::yahoo::{YahooClient, YahooError, YahooHosts, SIGNED_OUT};
use draft_assistant_lib::yahoo_oauth::{AuthError, OauthClient, TokenSet, YahooCredentials, OOB};
use yahoo_stub::{serve, Hits, Reply, Request, Stub};

const TEAMS: &str = include_str!("../fixtures/yahoo/teams.json");
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
