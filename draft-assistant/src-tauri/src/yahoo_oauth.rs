//! Yahoo's OAuth 2.0 authorization-code flow.
//!
//! Yahoo has no public-client/PKCE mode for the Fantasy API: the token
//! endpoint wants `Authorization: Basic base64(client_id:client_secret)`, so a
//! desktop app carries the secret. Everything here therefore treats the secret
//! as poison — it is never logged, never put in a URL, and never reproduced in
//! an error (see [`redact`], which scrubs it out of any Yahoo error body
//! before it can reach a message the user or a log file will see).
//!
//! Two redirect styles are supported, both allowed by Yahoo:
//!
//! - [`OOB`] (the default): Yahoo shows the user a code to paste back in. No
//!   listener, no port, nothing to register beyond the app itself.
//! - a loopback URI (`http://localhost:<port>`): [`catch_redirect`] binds the
//!   port, takes the one request the browser makes, and answers with a page
//!   telling the user to close the tab.
//!
//! The base64 encoder is eight lines at the bottom of this file rather than a
//! new dependency; it is exercised against the RFC 4648 vectors.

use serde::{Deserialize, Serialize};
use std::time::Duration;

/// The loopback listener. Split out to keep this file about the token dance;
/// re-exported below so that `yahoo_oauth::catch_redirect` still names it.
#[path = "yahoo_redirect.rs"]
pub mod redirect;

pub use redirect::{
    catch_redirect, catch_redirect_on, catch_redirect_on_within, catch_redirect_within,
    parse_redirect, Redirect, REDIRECT_WAIT,
};

/// Yahoo's login host. Both OAuth endpoints hang off it.
pub const LOGIN_BASE: &str = "https://api.login.yahoo.com";
/// The "show the user a code to paste" redirect.
pub const OOB: &str = "oob";
/// Yahoo Fantasy, read only. Asking for it by name is what keeps the token
/// this flow issues from being able to write to anybody's league.
pub const SCOPE: &str = "fspt-r";
/// An access token is refreshed this many seconds before it actually expires,
/// so a request cannot leave with a token that dies in flight.
pub const SKEW: u64 = 60;

/// The credentials Yahoo issues when an app is registered. The secret lives in
/// the Keychain (see [`crate::yahoo_secrets`]) and travels no further than the
/// `Authorization` header this module builds.
#[derive(Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct YahooCredentials {
    pub client_id: String,
    pub client_secret: String,
}

/// Written by hand rather than derived: a derived `Debug` would put the secret
/// into any `{:?}`, and one `dbg!` in a panic message or a log line is all it
/// would take to spill it. Neither half is shown — the id identifies the
/// registered app, which is enough to pair a leaked secret with.
impl std::fmt::Debug for YahooCredentials {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("YahooCredentials")
            .field("client_id", &"<redacted>")
            .field("client_secret", &"<redacted>")
            .finish()
    }
}

/// A token pair, plus the wall-clock second the access token stops working.
///
/// Yahoo's refresh tokens do not expire — they survive password changes — so
/// this is the whole of what has to be persisted between runs.
#[derive(Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct TokenSet {
    pub access_token: String,
    pub refresh_token: String,
    /// Epoch seconds.
    pub expires_at: u64,
}

/// Redacted for the same reason [`YahooCredentials`]'s is: a token is a
/// password. The expiry stays, because that is the field a failing test or a
/// log line actually wants to see.
impl std::fmt::Debug for TokenSet {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("TokenSet")
            .field("access_token", &"<redacted>")
            .field("refresh_token", &"<redacted>")
            .field("expires_at", &self.expires_at)
            .finish()
    }
}

impl TokenSet {
    /// Whether the access token is gone, or close enough to gone ([`SKEW`]
    /// seconds) that a request using it might not land in time.
    pub fn is_expired(&self, now: u64) -> bool {
        self.expires_at <= now.saturating_add(SKEW)
    }
}

/// Yahoo's reply to `/oauth2/get_token`. No `Debug`: it is a token pair
/// before it becomes a [`TokenSet`], and just as worth not printing.
#[derive(Clone, Deserialize)]
struct TokenResponse {
    access_token: String,
    #[serde(default)]
    refresh_token: Option<String>,
    #[serde(default)]
    expires_in: Option<u64>,
}

/// What an OAuth step can fail with. No variant carries the client secret;
/// bodies that come back from Yahoo pass through [`redact`] first.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum AuthError {
    /// The caller's input could not be used (empty code, unusable port).
    Invalid(String),
    /// A non-success status from the token endpoint.
    Http { status: u16, detail: String },
    /// The request never completed.
    Transport(String),
    /// The reply arrived but was not a token.
    Decode(String),
}

impl std::fmt::Display for AuthError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            AuthError::Invalid(message) => f.write_str(message),
            AuthError::Http { status, detail } => {
                write!(f, "Yahoo login returned HTTP {status}: {detail}")
            }
            AuthError::Transport(detail) => write!(f, "Yahoo login unreachable: {detail}"),
            AuthError::Decode(detail) => write!(f, "Yahoo login sent an unusable reply: {detail}"),
        }
    }
}

impl std::error::Error for AuthError {}

/// The browser URL that starts the flow.
///
/// `state` is echoed back on the redirect; [`catch_redirect`] hands it to the
/// caller to compare, which is what stops a stray request to the loopback port
/// from being taken for the real one.
pub fn authorize_url(client_id: &str, redirect_uri: &str, state: &str) -> String {
    authorize_url_on(LOGIN_BASE, client_id, redirect_uri, state)
}

/// [`authorize_url`] against a different login host, for tests.
pub fn authorize_url_on(base: &str, client_id: &str, redirect_uri: &str, state: &str) -> String {
    format!(
        "{base}/oauth2/request_auth?client_id={}&redirect_uri={}&response_type=code&scope={}&state={}&language=en-us",
        encode(client_id),
        encode(redirect_uri),
        encode(SCOPE),
        encode(state)
    )
}

/// Percent-encode everything that is not unreserved. Small enough to keep
/// here, and it means a client id with an underscore-dash-dot alphabet (which
/// is what Yahoo issues) is never mangled.
fn encode(raw: &str) -> String {
    let mut out = String::with_capacity(raw.len());
    for byte in raw.bytes() {
        match byte {
            b'A'..=b'Z' | b'a'..=b'z' | b'0'..=b'9' | b'-' | b'.' | b'_' | b'~' => {
                out.push(byte as char)
            }
            _ => out.push_str(&format!("%{byte:02X}")),
        }
    }
    out
}

/// The `Authorization` header value for the token endpoint.
pub fn basic_header(credentials: &YahooCredentials) -> String {
    format!(
        "Basic {}",
        base64(format!("{}:{}", credentials.client_id, credentials.client_secret).as_bytes())
    )
}

/// Remove the secret from anything on its way into an error or a log.
///
/// Yahoo does not echo the secret today, but the body of a 4xx is
/// attacker-influenced text that ends up in a message the user can copy; the
/// cost of being sure is one string scan.
pub fn redact(detail: &str, secret: &str) -> String {
    if secret.is_empty() {
        return detail.to_string();
    }
    detail.replace(secret, "***")
}

/// The form body for one token request. Pure, so the wire shape is pinned by a
/// test rather than by reading the code.
pub fn token_form(kind: Grant<'_>, redirect_uri: &str) -> Vec<(String, String)> {
    let mut form = vec![("redirect_uri".to_string(), redirect_uri.to_string())];
    match kind {
        Grant::Code(code) => {
            form.push(("grant_type".into(), "authorization_code".into()));
            form.push(("code".into(), code.to_string()));
        }
        Grant::Refresh(token) => {
            form.push(("grant_type".into(), "refresh_token".into()));
            form.push(("refresh_token".into(), token.to_string()));
        }
    }
    form
}

/// Which of the two token requests is being made.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Grant<'a> {
    Code(&'a str),
    Refresh(&'a str),
}

/// Talks to Yahoo's login host. Separate from [`crate::yahoo::YahooClient`]
/// because the two speak to different hosts with different auth.
pub struct OauthClient {
    http: reqwest::Client,
    base: String,
}

impl Default for OauthClient {
    fn default() -> Self {
        Self::new()
    }
}

impl OauthClient {
    pub fn new() -> Self {
        Self::with_base(LOGIN_BASE)
    }

    /// A client pointed at `base` instead of Yahoo — how the wire tests reach
    /// a stub. Proxies are ignored for the same reason `SleeperClient` ignores
    /// them: a named destination should be the destination.
    pub fn with_base(base: impl Into<String>) -> Self {
        let http = reqwest::Client::builder()
            .user_agent(crate::yahoo::USER_AGENT)
            .no_proxy()
            .connect_timeout(Duration::from_secs(3))
            .timeout(Duration::from_secs(10))
            .build()
            .expect("failed to build http client");
        Self {
            http,
            base: base.into(),
        }
    }

    /// Swap the authorization code for a token pair.
    pub async fn exchange_code(
        &self,
        credentials: &YahooCredentials,
        code: &str,
        redirect_uri: &str,
    ) -> Result<TokenSet, AuthError> {
        let code = code.trim();
        if code.is_empty() {
            return Err(AuthError::Invalid("no authorization code was given".into()));
        }
        self.token(credentials, Grant::Code(code), redirect_uri, None)
            .await
    }

    /// Trade the refresh token for a fresh access token.
    ///
    /// Yahoo usually returns the same refresh token; when it omits one the old
    /// one stays valid, so it is carried over rather than lost.
    pub async fn refresh(
        &self,
        credentials: &YahooCredentials,
        refresh_token: &str,
        redirect_uri: &str,
    ) -> Result<TokenSet, AuthError> {
        if refresh_token.trim().is_empty() {
            return Err(AuthError::Invalid("no refresh token is stored".into()));
        }
        self.token(
            credentials,
            Grant::Refresh(refresh_token),
            redirect_uri,
            Some(refresh_token),
        )
        .await
    }

    async fn token(
        &self,
        credentials: &YahooCredentials,
        grant: Grant<'_>,
        redirect_uri: &str,
        carry_over: Option<&str>,
    ) -> Result<TokenSet, AuthError> {
        let url = format!("{}/oauth2/get_token", self.base);
        let response = self
            .http
            .post(&url)
            .header(reqwest::header::AUTHORIZATION, basic_header(credentials))
            .form(&token_form(grant, redirect_uri))
            .send()
            .await
            .map_err(|e| {
                AuthError::Transport(redact(&e.to_string(), &credentials.client_secret))
            })?;
        let status = response.status();
        let body = response.text().await.unwrap_or_default();
        let body = redact(&body, &credentials.client_secret);
        if !status.is_success() {
            return Err(AuthError::Http {
                status: status.as_u16(),
                detail: trim_detail(&body),
            });
        }
        let parsed: TokenResponse = serde_json::from_str(&body)
            .map_err(|e| AuthError::Decode(redact(&e.to_string(), &credentials.client_secret)))?;
        Ok(token_set(parsed, carry_over, now_secs()))
    }
}

/// Keep an error body short enough to show, and on one line.
fn trim_detail(body: &str) -> String {
    let flat: String = body.split_whitespace().collect::<Vec<_>>().join(" ");
    if flat.chars().count() > 200 {
        flat.chars().take(200).collect::<String>() + "..."
    } else {
        flat
    }
}

/// Build the stored token pair from Yahoo's reply. Pure so that expiry
/// arithmetic and the carried-over refresh token are testable without a clock.
fn token_set(response: TokenResponse, carry_over: Option<&str>, now: u64) -> TokenSet {
    TokenSet {
        access_token: response.access_token,
        refresh_token: response
            .refresh_token
            .filter(|token| !token.is_empty())
            .or_else(|| carry_over.map(str::to_string))
            .unwrap_or_default(),
        // Yahoo sends 3600. A reply without one is treated as already old, so
        // the next call refreshes rather than trusting an unknown lifetime.
        expires_at: now.saturating_add(response.expires_in.unwrap_or(0)),
    }
}

pub fn now_secs() -> u64 {
    std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map(|d| d.as_secs())
        .unwrap_or(0)
}

/// Standard base64 with padding (RFC 4648 §4). Present so that the one place
/// this app needs base64 does not cost it a dependency.
pub fn base64(input: &[u8]) -> String {
    const ALPHABET: &[u8; 64] = b"ABCDEFGHIJKLMNOPQRSTUVWXYZabcdefghijklmnopqrstuvwxyz0123456789+/";
    let mut out = String::with_capacity(input.len().div_ceil(3) * 4);
    for group in input.chunks(3) {
        let bits = (u32::from(group[0]) << 16)
            | (group.get(1).copied().map_or(0, u32::from) << 8)
            | group.get(2).copied().map_or(0, u32::from);
        for slot in 0..4 {
            if slot <= group.len() {
                out.push(ALPHABET[((bits >> (18 - 6 * slot)) & 0x3F) as usize] as char);
            } else {
                out.push('=');
            }
        }
    }
    out
}

#[cfg(test)]
#[path = "yahoo_oauth_tests.rs"]
mod tests;
