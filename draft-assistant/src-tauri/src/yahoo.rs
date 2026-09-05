//! Read-only client for the Yahoo Fantasy API.
//!
//! Same transport policy as [`crate::sleeper`] — pooled reqwest client, gzip,
//! 3s connect / 8s total, several attempts for the failures that a retry could
//! fix — plus the three things Yahoo adds:
//!
//! - every request carries `Authorization: Bearer <access token>`,
//! - an access token lasts an hour, so the client refreshes it when it is
//!   about to expire and once more if Yahoo answers 401 anyway (a token can
//!   be revoked well before its clock runs out), and
//! - Yahoo throttles hard and says so through `Retry-After`, so the waits
//!   between attempts come from [`crate::yahoo_retry`] rather than from a
//!   fixed pair of milliseconds.
//!
//! Only one refresh is ever in flight. A pool load fires seven requests at
//! once, and when the access token has just expired every one of them used to
//! spend the refresh token in turn; the first one through the gate now
//! refreshes and the rest read what it left behind.
//!
//! After any call, [`YahooClient::tokens`] hands back the current pair; the
//! caller persists it so the refresh survives a restart. The client does not
//! write to the Keychain itself — that is [`crate::yahoo_secrets`]'s job, and
//! keeping it out of here is what lets these tests run without one.
//!
//! Responses come back as JSON only because every path here appends
//! `?format=json`; without it Yahoo serves XML.

use crate::yahoo_oauth::{AuthError, OauthClient, TokenSet, YahooCredentials, LOGIN_BASE, OOB};
use crate::yahoo_retry::{retry_after, RetryPolicy};
use crate::yahoo_types::{PlayerPage, YahooDraftPick, YahooLeague, YahooTeam};
use serde_json::Value;
use std::time::Duration;
use tokio::sync::Mutex;

/// The documented v2 root.
pub const BASE: &str = "https://fantasysports.yahooapis.com/fantasy/v2";
/// Shared with [`crate::yahoo_oauth`] so both hosts see one identity.
pub const USER_AGENT: &str = "draft-assistant/0.1 (local second-screen tool)";
/// The NFL game key. `nfl` resolves to the current season's numeric key.
pub const NFL: &str = "nfl";
/// Yahoo's own page ceiling for a players query.
pub const PAGE: u32 = 25;

/// A failed read from Yahoo.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum YahooError {
    /// The request was never made: a key that cannot go in a URL.
    Invalid(String),
    /// The token could not be obtained or renewed.
    Auth(AuthError),
    Http {
        status: u16,
        url: String,
    },
    Transport {
        url: String,
        detail: String,
    },
    Decode {
        url: String,
        detail: String,
    },
}

/// One failed attempt: the error, and how long Yahoo asked to be left alone
/// for. Internal — `Retry-After` is a fact about this attempt rather than
/// about the error, and it would be noise on [`YahooError`], which is what
/// the user is eventually shown.
struct Failure {
    error: YahooError,
    asked_for: Option<Duration>,
}

impl Failure {
    fn plain(error: YahooError) -> Self {
        Self {
            error,
            asked_for: None,
        }
    }
}

/// Yahoo answers a throttled caller with its own status 999 rather than the
/// documented 429. Both mean the same thing and both clear on their own.
pub const RATE_LIMITED: [u16; 2] = [429, 999];

impl YahooError {
    /// Whether repeating the identical request could plausibly succeed.
    pub fn retryable(&self) -> bool {
        match self {
            YahooError::Transport { .. } => true,
            YahooError::Http { status, .. } => {
                (500..600).contains(status) || RATE_LIMITED.contains(status)
            }
            YahooError::Invalid(_) | YahooError::Auth(_) | YahooError::Decode { .. } => false,
        }
    }
}

impl std::fmt::Display for YahooError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            YahooError::Invalid(message) => f.write_str(message),
            YahooError::Auth(error) => write!(f, "{error}"),
            // "HTTP 999" is Yahoo's, and means nothing to anybody; the one
            // thing the user can do about it is wait, so say that instead.
            YahooError::Http { status, .. } if RATE_LIMITED.contains(status) => {
                f.write_str("Yahoo is rate-limiting requests — try again in a minute")
            }
            YahooError::Http { status, url } => write!(f, "HTTP {status} for {url}"),
            YahooError::Transport { url, detail } => write!(f, "request failed: {url}: {detail}"),
            YahooError::Decode { url, detail } => write!(f, "bad JSON from {url}: {detail}"),
        }
    }
}

impl std::error::Error for YahooError {}

/// Where a client's two hosts point. Overridable so a test can serve both the
/// fantasy API and the login host from one stub socket.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct YahooHosts {
    pub api_base: String,
    pub login_base: String,
    /// The redirect URI the tokens were issued against; Yahoo wants it
    /// repeated on every token request, refreshes included.
    pub redirect_uri: String,
}

impl Default for YahooHosts {
    fn default() -> Self {
        Self {
            api_base: BASE.to_string(),
            login_base: LOGIN_BASE.to_string(),
            redirect_uri: OOB.to_string(),
        }
    }
}

/// A key that is safe to interpolate into a path: `449.l.12345.t.7`.
fn check_key(kind: &str, key: &str) -> Result<(), YahooError> {
    let ok = !key.is_empty()
        && key.len() <= 64
        && key
            .chars()
            .all(|c| c.is_ascii_alphanumeric() || matches!(c, '.' | '_' | '-'));
    if ok {
        Ok(())
    } else {
        Err(YahooError::Invalid(format!(
            "'{key}' is not a valid Yahoo {kind} key"
        )))
    }
}

/// The full URL for one API path, always asking for JSON.
pub fn url_for(base: &str, path: &str) -> String {
    let separator = if path.contains('?') { '&' } else { '?' };
    format!("{base}{path}{separator}format=json")
}

pub struct YahooClient {
    http: reqwest::Client,
    pub(crate) hosts: YahooHosts,
    oauth: OauthClient,
    credentials: YahooCredentials,
    /// Held only long enough to read or replace the pair. Never across a
    /// request: a lock held over the network is how one slow refresh used to
    /// stall every other call the load had in flight.
    tokens: Mutex<TokenSet>,
    /// The one caller allowed to be refreshing at any moment.
    refresh_gate: Mutex<()>,
    retry: RetryPolicy,
}

impl YahooClient {
    /// A client against the real Yahoo, holding the tokens it was given.
    pub fn new(credentials: YahooCredentials, tokens: TokenSet) -> Self {
        Self::with_hosts(credentials, tokens, YahooHosts::default())
    }

    /// A client whose hosts are named exactly, ignoring `HTTP_PROXY` for the
    /// same reason [`crate::sleeper::SleeperClient::with_host`] does.
    pub fn with_hosts(credentials: YahooCredentials, tokens: TokenSet, hosts: YahooHosts) -> Self {
        Self::with_hosts_timeout(credentials, tokens, hosts, Duration::from_secs(8))
    }

    /// [`Self::with_hosts`] with a request timeout of the caller's choosing.
    ///
    /// Only a test wants this: it is how the "the server accepted and then
    /// said nothing" path is exercised without the suite sitting through the
    /// eight seconds the app itself waits.
    pub fn with_hosts_timeout(
        credentials: YahooCredentials,
        tokens: TokenSet,
        hosts: YahooHosts,
        timeout: Duration,
    ) -> Self {
        let http = reqwest::Client::builder()
            .user_agent(USER_AGENT)
            .gzip(true)
            .no_proxy()
            .connect_timeout(Duration::from_secs(3))
            .timeout(timeout)
            .build()
            .expect("failed to build http client");
        let oauth = OauthClient::with_base(hosts.login_base.clone());
        Self {
            http,
            hosts,
            oauth,
            credentials,
            tokens: Mutex::new(tokens),
            refresh_gate: Mutex::new(()),
            retry: RetryPolicy::default(),
        }
    }

    /// The same client with a different retry policy.
    ///
    /// Only a test wants this: the real waits run to sixteen seconds, and a
    /// test that asserts on the number of attempts should not sit through
    /// half a minute of them.
    pub fn with_retry(mut self, retry: RetryPolicy) -> Self {
        self.retry = retry;
        self
    }

    /// The token pair as it stands, including anything a refresh changed.
    /// Persist this after a call to keep the refresh across restarts.
    pub async fn tokens(&self) -> TokenSet {
        self.tokens.lock().await.clone()
    }

    /// A usable access token, renewing first if the stored one is expired or
    /// within the skew window of expiring.
    async fn access_token(&self) -> Result<String, YahooError> {
        {
            let tokens = self.tokens.lock().await;
            if !tokens.is_expired(crate::yahoo_oauth::now_secs()) {
                return Ok(tokens.access_token.clone());
            }
        }
        self.renew().await
    }

    /// Spend the refresh token, once, however many callers want one.
    ///
    /// The gate is a separate lock from the token pair so that nothing holds
    /// the pair across the ten-second round trip. Whoever gets through it
    /// looks again first: a caller that queued behind a refresh wants the
    /// token that refresh produced, not a second refresh of its own — and a
    /// refresh token Yahoo has already rotated away would be spent for
    /// nothing, signing the user out mid-draft.
    async fn renew(&self) -> Result<String, YahooError> {
        let stale = self.tokens.lock().await.access_token.clone();
        let _gate = self.refresh_gate.lock().await;
        let refresh_token = {
            let tokens = self.tokens.lock().await;
            if tokens.access_token != stale && !tokens.is_expired(crate::yahoo_oauth::now_secs()) {
                return Ok(tokens.access_token.clone());
            }
            tokens.refresh_token.clone()
        };
        let fresh = self
            .oauth
            .refresh(&self.credentials, &refresh_token, &self.hosts.redirect_uri)
            .await
            .map_err(YahooError::Auth)?;
        let access = fresh.access_token.clone();
        *self.tokens.lock().await = fresh;
        Ok(access)
    }

    async fn get_once(&self, url: &str, token: &str) -> Result<String, Failure> {
        let response = self.http.get(url).bearer_auth(token).send().await;
        let response = match response {
            Ok(response) => response,
            Err(e) => {
                return Err(Failure::plain(YahooError::Transport {
                    url: url.to_string(),
                    detail: e.to_string(),
                }))
            }
        };
        let status = response.status();
        if !status.is_success() {
            // Read the header before the body is dropped: it is the only
            // thing that says how long Yahoo's throttle has left to run.
            let asked_for = response
                .headers()
                .get(reqwest::header::RETRY_AFTER)
                .and_then(|value| value.to_str().ok())
                .and_then(|value| retry_after(value, crate::yahoo_oauth::now_secs()));
            return Err(Failure {
                error: YahooError::Http {
                    status: status.as_u16(),
                    url: url.to_string(),
                },
                asked_for,
            });
        }
        response.text().await.map_err(|e| {
            Failure::plain(YahooError::Transport {
                url: url.to_string(),
                detail: e.to_string(),
            })
        })
    }

    /// One GET, with the retry policy and exactly one refresh-and-retry.
    ///
    /// A 401 is not counted as an attempt: it is answered by renewing the
    /// token and going again immediately, and only once — a second 401 means
    /// the grant is gone, and repeating it would only spend the refresh token
    /// against a door that is closed.
    async fn get_body(&self, url: &str) -> Result<String, YahooError> {
        let mut attempts = 0;
        let mut refreshed = false;
        loop {
            let token = self.access_token().await?;
            match self.get_once(url, &token).await {
                Ok(body) => return Ok(body),
                Err(failure)
                    if matches!(failure.error, YahooError::Http { status: 401, .. })
                        && !refreshed =>
                {
                    refreshed = true;
                    self.renew().await?;
                }
                Err(failure) => {
                    attempts += 1;
                    if !failure.error.retryable() || attempts >= self.retry.attempts {
                        return Err(failure.error);
                    }
                    tokio::time::sleep(self.retry.wait(attempts, failure.asked_for)).await;
                }
            }
        }
    }

    /// A GET whose body is parsed into `T`. `path` is everything after the
    /// `/fantasy/v2` root, starting with a slash; `?format=json` is appended.
    pub async fn get_json<T: serde::de::DeserializeOwned>(
        &self,
        path: &str,
    ) -> Result<T, YahooError> {
        let url = url_for(&self.hosts.api_base, path);
        let body = self.get_body(&url).await?;
        serde_json::from_str(&body).map_err(|e| YahooError::Decode {
            url,
            detail: e.to_string(),
        })
    }

    /// The raw JSON value, which is what every parser here actually wants:
    /// Yahoo's shape is too irregular to deserialize straight onto a struct.
    pub async fn get_value(&self, path: &str) -> Result<Value, YahooError> {
        self.get_json::<Value>(path).await
    }

    /// Every league the logged-in user plays in for one game (`nfl`, or a
    /// season-specific key like `449`).
    pub async fn user_leagues(&self, game_key: &str) -> Result<Vec<YahooLeague>, YahooError> {
        check_key("game", game_key)?;
        let value = self
            .get_value(&format!(
                "/users;use_login=1/games;game_keys={game_key}/leagues"
            ))
            .await?;
        Ok(crate::yahoo_parse::user_leagues(&value))
    }

    /// One league, settings included — roster slots and scoring rules come
    /// from `/settings` rather than the bare league resource.
    pub async fn league(&self, league_key: &str) -> Result<YahooLeague, YahooError> {
        check_key("league", league_key)?;
        let value = self
            .get_value(&format!("/league/{league_key}/settings"))
            .await?;
        crate::yahoo_parse::league(&value)
            .ok_or_else(|| YahooError::Invalid(format!("league {league_key} was not in the reply")))
    }

    pub async fn league_teams(&self, league_key: &str) -> Result<Vec<YahooTeam>, YahooError> {
        check_key("league", league_key)?;
        let value = self
            .get_value(&format!("/league/{league_key}/teams"))
            .await?;
        Ok(crate::yahoo_parse::teams(&value))
    }

    /// The picks made so far. Empty before the draft, partial during it.
    pub async fn draft_results(&self, league_key: &str) -> Result<Vec<YahooDraftPick>, YahooError> {
        check_key("league", league_key)?;
        let value = self
            .get_value(&format!("/league/{league_key}/draftresults"))
            .await?;
        Ok(crate::yahoo_parse::draft_results(&value))
    }

    /// One page of the league's player pool. `position` filters to `QB`, `WR`,
    /// `DEF`, ... when given.
    pub async fn players(
        &self,
        league_key: &str,
        start: u32,
        count: u32,
        position: Option<&str>,
    ) -> Result<PlayerPage, YahooError> {
        check_key("league", league_key)?;
        let mut path = format!("/league/{league_key}/players;start={start};count={count}");
        if let Some(position) = position {
            check_key("position", position)?;
            path.push_str(&format!(";position={position}"));
        }
        let value = self.get_value(&path).await?;
        Ok(crate::yahoo_parse::players(&value))
    }

    /// The players currently on one team.
    pub async fn team_roster(
        &self,
        team_key: &str,
    ) -> Result<Vec<crate::yahoo_types::YahooPlayer>, YahooError> {
        check_key("team", team_key)?;
        let value = self.get_value(&format!("/team/{team_key}/roster")).await?;
        Ok(crate::yahoo_parse::players(&value).players)
    }
}

#[cfg(test)]
#[path = "yahoo_tests.rs"]
mod tests;
