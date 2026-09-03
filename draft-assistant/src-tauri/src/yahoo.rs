//! Read-only client for the Yahoo Fantasy API.
//!
//! Same transport policy as [`crate::sleeper`] — pooled reqwest client, gzip,
//! 3s connect / 8s total, three attempts with a doubling backoff for the
//! failures that a retry could fix — plus the two things Yahoo adds:
//!
//! - every request carries `Authorization: Bearer <access token>`, and
//! - an access token lasts an hour, so the client refreshes it when it is
//!   about to expire and once more if Yahoo answers 401 anyway (a token can
//!   be revoked well before its clock runs out).
//!
//! After any call, [`YahooClient::tokens`] hands back the current pair; the
//! caller persists it so the refresh survives a restart. The client does not
//! write to the Keychain itself — that is [`crate::yahoo_secrets`]'s job, and
//! keeping it out of here is what lets these tests run without one.
//!
//! Responses come back as JSON only because every path here appends
//! `?format=json`; without it Yahoo serves XML.

use crate::yahoo_oauth::{AuthError, OauthClient, TokenSet, YahooCredentials, LOGIN_BASE, OOB};
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
/// Total attempts per request, including the first.
const RETRIES: u32 = 3;
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

impl YahooError {
    /// Whether repeating the identical request could plausibly succeed.
    pub fn retryable(&self) -> bool {
        match self {
            YahooError::Transport { .. } => true,
            YahooError::Http { status, .. } => (500..600).contains(status),
            YahooError::Invalid(_) | YahooError::Auth(_) | YahooError::Decode { .. } => false,
        }
    }
}

impl std::fmt::Display for YahooError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            YahooError::Invalid(message) => f.write_str(message),
            YahooError::Auth(error) => write!(f, "{error}"),
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
    hosts: YahooHosts,
    oauth: OauthClient,
    credentials: YahooCredentials,
    tokens: Mutex<TokenSet>,
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
        }
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

    /// Spend the refresh token. Held across the request so two callers cannot
    /// both refresh with the same token and race each other's result.
    async fn renew(&self) -> Result<String, YahooError> {
        let mut tokens = self.tokens.lock().await;
        let fresh = self
            .oauth
            .refresh(
                &self.credentials,
                &tokens.refresh_token,
                &self.hosts.redirect_uri,
            )
            .await
            .map_err(YahooError::Auth)?;
        let access = fresh.access_token.clone();
        *tokens = fresh;
        Ok(access)
    }

    async fn get_once(&self, url: &str, token: &str) -> Result<String, YahooError> {
        let response = self
            .http
            .get(url)
            .bearer_auth(token)
            .send()
            .await
            .map_err(|e| YahooError::Transport {
                url: url.to_string(),
                detail: e.to_string(),
            })?;
        let status = response.status();
        if !status.is_success() {
            return Err(YahooError::Http {
                status: status.as_u16(),
                url: url.to_string(),
            });
        }
        response.text().await.map_err(|e| YahooError::Transport {
            url: url.to_string(),
            detail: e.to_string(),
        })
    }

    /// One GET, with the retry policy and exactly one refresh-and-retry.
    ///
    /// A 401 is not counted as an attempt: it is answered by renewing the
    /// token and going again immediately, and only once — a second 401 means
    /// the grant is gone, and repeating it would only spend the refresh token
    /// against a door that is closed.
    async fn get_body(&self, url: &str) -> Result<String, YahooError> {
        let mut backoff = Duration::from_millis(250);
        let mut attempts = 0;
        let mut refreshed = false;
        loop {
            let token = self.access_token().await?;
            match self.get_once(url, &token).await {
                Ok(body) => return Ok(body),
                Err(YahooError::Http { status: 401, .. }) if !refreshed => {
                    refreshed = true;
                    self.renew().await?;
                }
                Err(error) => {
                    attempts += 1;
                    if !error.retryable() || attempts == RETRIES {
                        return Err(error);
                    }
                    tokio::time::sleep(backoff).await;
                    backoff *= 2;
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

    /// Every player Yahoo will hand over, one page at a time.
    ///
    /// Yahoo reports no total, so the end is a page that comes back shorter
    /// than it was asked for. `limit` is a stop of last resort: without it a
    /// server that kept answering with full pages would page forever.
    pub async fn all_players(
        &self,
        league_key: &str,
        position: Option<&str>,
        limit: u32,
    ) -> Result<Vec<crate::yahoo_types::YahooPlayer>, YahooError> {
        let mut all = Vec::new();
        let mut start = 0;
        while start < limit {
            let page = self.players(league_key, start, PAGE, position).await?;
            let fetched = page.players.len() as u32;
            all.extend(page.players);
            if fetched < PAGE {
                break;
            }
            start += PAGE;
        }
        Ok(all)
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
mod tests {
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
}
