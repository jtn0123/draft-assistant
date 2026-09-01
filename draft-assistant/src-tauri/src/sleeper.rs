//! Read-only Sleeper API client.
//!
//! Everything here is unauthenticated GETs against api.sleeper.app.
//! The projections endpoints are undocumented, so every response is
//! deserialized defensively (unknown fields ignored, missing fields defaulted)
//! and raw JSON snapshots are cached on disk by the caller.

use crate::sleeper_error::SleeperError;
use serde::{Deserialize, Serialize};
use std::collections::HashMap;
use std::time::Duration;

/// The documented v1 API root. Declared once here; `season_api` imports it
/// rather than repeating the host. Both roots are rewritten on the way out by
/// `sleeper_host::route`, which is how a debug build can be pointed at the
/// replay server instead.
pub(crate) const BASE: &str = "https://api.sleeper.app/v1";
/// Root for the undocumented endpoints (projections, scores).
pub(crate) const BASE_UNDOC: &str = "https://api.sleeper.app";
/// Total attempts per request, including the first.
const RETRIES: u32 = 3;

/// A Sleeper account, as returned by `/v1/user/{username}`.
#[derive(Debug, Clone, Deserialize)]
pub struct SleeperUser {
    pub user_id: String,
}

/// League-wide knobs the season screen needs. All optional: a mock draft's
/// synthesized league has none of them.
#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct LeagueSettings {
    /// First week of the playoffs; the regular season is weeks 1..this.
    #[serde(default)]
    pub playoff_week_start: Option<u32>,
    #[serde(default)]
    pub playoff_teams: Option<u32>,
    /// FAAB budget. Absent on leagues using waiver priority instead.
    #[serde(default)]
    pub waiver_budget: Option<f64>,
    #[serde(default)]
    pub start_week: Option<u32>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct League {
    pub league_id: String,
    pub name: String,
    pub season: String,
    pub status: String,
    pub total_rosters: u32,
    pub roster_positions: Vec<String>,
    pub scoring_settings: HashMap<String, f64>,
    pub draft_id: Option<String>,
    /// Same league, prior season — the "Last season" tab reads this.
    #[serde(default)]
    pub previous_league_id: Option<String>,
    #[serde(default)]
    pub settings: LeagueSettings,
}

impl League {
    /// Weeks 1..=this are regular-season matchups. Sleeper's default is 15.
    pub fn last_regular_week(&self) -> u32 {
        self.settings
            .playoff_week_start
            .filter(|w| *w > 1)
            .unwrap_or(15)
            .saturating_sub(1)
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct DraftSettings {
    pub teams: u32,
    pub rounds: u32,
    #[serde(default)]
    pub pick_timer: Option<u32>,
    /// Snake drafts only: the round from which the order reverses a second
    /// time ("third-round reversal" = 3). 0 or absent = plain snake.
    #[serde(default)]
    pub reversal_round: Option<u32>,
    // Roster shape, present on mock drafts (which have no league to read it
    // from). All optional: league drafts carry it too but we prefer the league.
    #[serde(default)]
    pub slots_qb: Option<u32>,
    #[serde(default)]
    pub slots_rb: Option<u32>,
    #[serde(default)]
    pub slots_wr: Option<u32>,
    #[serde(default)]
    pub slots_te: Option<u32>,
    #[serde(default)]
    pub slots_flex: Option<u32>,
    #[serde(default)]
    pub slots_super_flex: Option<u32>,
    #[serde(default)]
    pub slots_k: Option<u32>,
    #[serde(default)]
    pub slots_def: Option<u32>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct DraftMetadata {
    #[serde(default)]
    pub name: Option<String>,
    /// "std" | "half_ppr" | "ppr" — only meaningful on leagueless mock drafts.
    #[serde(default)]
    pub scoring_type: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Draft {
    pub draft_id: String,
    pub status: String,
    #[serde(rename = "type")]
    pub draft_type: String,
    pub settings: DraftSettings,
    /// user_id -> draft slot (1-based)
    #[serde(default)]
    pub draft_order: Option<HashMap<String, u32>>,
    #[serde(default)]
    pub start_time: Option<i64>,
    #[serde(default)]
    pub season: Option<String>,
    #[serde(default)]
    pub metadata: Option<DraftMetadata>,
    /// User ids that created the draft (mock drafts may use a guest id here).
    #[serde(default)]
    pub creators: Option<Vec<String>>,
    /// Epoch milliseconds of the most recent pick; with `settings.pick_timer`
    /// this gives the current pick's deadline.
    #[serde(default)]
    pub last_picked: Option<u64>,
    /// draft slot (1-based, string key) -> league roster id: the only bridge
    /// between slots and the roster ids traded picks are recorded against.
    #[serde(default)]
    pub slot_to_roster_id: Option<HashMap<String, u32>>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Pick {
    pub round: u32,
    /// overall pick number, 1-based
    pub pick_no: u32,
    /// draft slot that made the pick, 1-based
    pub draft_slot: u32,
    pub player_id: String,
    #[serde(default)]
    pub picked_by: Option<String>,
    #[serde(default)]
    pub metadata: Option<PickMeta>,
    /// Sleeper's keeper flag, and only a hint — it arrives null on plenty of
    /// genuine keepers. See `crate::picks::keeper_pick_nos` for the real test.
    #[serde(default)]
    pub is_keeper: Option<bool>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct PickMeta {
    #[serde(default)]
    pub first_name: Option<String>,
    #[serde(default)]
    pub last_name: Option<String>,
    #[serde(default)]
    pub position: Option<String>,
    #[serde(default)]
    pub team: Option<String>,
}

/// One entry in the ~14MB players/nfl dictionary. Only what we need.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct PlayerMeta {
    #[serde(default)]
    pub full_name: Option<String>,
    #[serde(default)]
    pub first_name: Option<String>,
    #[serde(default)]
    pub last_name: Option<String>,
    #[serde(default)]
    pub position: Option<String>,
    #[serde(default)]
    pub team: Option<String>,
    #[serde(default)]
    pub fantasy_positions: Option<Vec<String>>,
    #[serde(default)]
    pub injury_status: Option<String>,
    #[serde(default)]
    pub years_exp: Option<u32>,
    #[serde(default)]
    pub age: Option<u32>,
}

/// One player's row from the undocumented projections endpoint.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ProjectionRow {
    pub player_id: String,
    /// Raw projected stat lines keyed by Sleeper stat names (pass_yd, rec, ...),
    /// plus ADP fields (adp_ppr, adp_half_ppr, ...). Same key space as
    /// league.scoring_settings, which is what makes re-scoring a dot product.
    #[serde(default)]
    pub stats: Option<HashMap<String, f64>>,
    #[serde(default)]
    pub player: Option<PlayerMeta>,
    #[serde(default)]
    pub week: Option<u32>,
    /// Weekly rows only: opposing team, `None` on the player's bye week.
    #[serde(default)]
    pub opponent: Option<String>,
}

impl ProjectionRow {
    pub fn stat(&self, key: &str) -> Option<f64> {
        self.stats.as_ref().and_then(|s| s.get(key).copied())
    }
}

#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct LeagueUserMeta {
    /// Custom team name. Users who never set one have no key here.
    #[serde(default)]
    pub team_name: Option<String>,
    /// Custom team picture, as a full sleepercdn URL.
    #[serde(default)]
    pub avatar: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct LeagueUser {
    pub user_id: String,
    #[serde(default)]
    pub display_name: Option<String>,
    /// Sleeper avatar hash for the account itself.
    #[serde(default)]
    pub avatar: Option<String>,
    #[serde(default)]
    pub metadata: Option<LeagueUserMeta>,
}

impl LeagueUser {
    /// What to call this team: their custom name, else their handle.
    pub fn label(&self) -> Option<String> {
        self.metadata
            .as_ref()
            .and_then(|m| m.team_name.clone())
            .filter(|n| !n.trim().is_empty())
            .or_else(|| self.display_name.clone())
    }

    /// The picture to draw for this team: their custom team image when they
    /// uploaded one, else their account avatar. `None` for the default egg.
    pub fn avatar_ref(&self) -> Option<String> {
        self.metadata
            .as_ref()
            .and_then(|m| m.avatar.clone())
            .filter(|a| !a.trim().is_empty())
            .or_else(|| self.avatar.clone())
            .filter(|a| !a.trim().is_empty())
    }
}

/// The one thing that speaks HTTP to Sleeper.
///
/// Its draft-facing surface is below. The in-season endpoints are declared as
/// a trait next to the types they return, so this list is the whole story:
///
/// - [`crate::season_api::SeasonEndpoints`] — NFL state, rosters, matchups,
///   transactions, the playoff bracket and the live scoreboard
pub struct SleeperClient {
    http: reqwest::Client,
}

impl Default for SleeperClient {
    fn default() -> Self {
        Self::new()
    }
}

impl SleeperClient {
    fn builder() -> reqwest::ClientBuilder {
        reqwest::Client::builder()
            .user_agent("draft-assistant/0.1 (local second-screen tool)")
            .gzip(true)
            .connect_timeout(Duration::from_secs(3))
            .timeout(Duration::from_secs(8))
    }

    pub fn new() -> Self {
        let http = Self::builder()
            .build()
            .expect("failed to build http client");
        Self { http }
    }

    /// A client that ignores `HTTP_PROXY`/`HTTPS_PROXY`. The offline tests set
    /// both, process-wide, to a dead port; transport tests that drive a real
    /// stub server on localhost must not be routed through that.
    #[cfg(test)]
    pub(crate) fn without_proxy() -> Self {
        let http = Self::builder()
            .no_proxy()
            .build()
            .expect("failed to build http client");
        Self { http }
    }

    /// The pooled HTTP client, reused by other callers (the chat panel) so
    /// the app keeps one connection pool rather than several.
    pub fn http_client(&self) -> reqwest::Client {
        self.http.clone()
    }

    /// One attempt, no retry. The returned `SleeperError` carries whether
    /// another try could help; `get_json` asks it rather than guessing.
    pub(crate) async fn get_json_once<T: serde::de::DeserializeOwned>(
        &self,
        url: &str,
    ) -> Result<T, SleeperError> {
        let resp = self
            .http
            .get(url)
            .send()
            .await
            .map_err(|e| SleeperError::Transport {
                url: url.to_string(),
                detail: e.to_string(),
            })?;
        let status = resp.status();
        if !status.is_success() {
            return Err(SleeperError::Http {
                status,
                url: url.to_string(),
            });
        }
        resp.json::<T>().await.map_err(|e| SleeperError::Decode {
            url: url.to_string(),
            detail: e.to_string(),
        })
    }

    /// One attempt that stops at the raw body, without deserialising it.
    ///
    /// The players dictionary is ~14.6 MB; turning it into a `HashMap` inside
    /// the task is hundreds of milliseconds during which no other task on the
    /// runtime moves. Handing back bytes lets the caller push that onto the
    /// blocking pool.
    async fn get_bytes_once(&self, url: &str) -> Result<Vec<u8>, SleeperError> {
        let resp = self
            .http
            .get(url)
            .send()
            .await
            .map_err(|e| SleeperError::Transport {
                url: url.to_string(),
                detail: e.to_string(),
            })?;
        let status = resp.status();
        if !status.is_success() {
            return Err(SleeperError::Http {
                status,
                url: url.to_string(),
            });
        }
        resp.bytes()
            .await
            .map(|body| body.to_vec())
            .map_err(|e| SleeperError::Transport {
                url: url.to_string(),
                detail: e.to_string(),
            })
    }

    /// Retry a request that survives a blip. Sleeper drops the occasional
    /// request during Sunday traffic, and a single failure used to blank a
    /// whole week of data until the next manual refresh.
    ///
    /// Only failures the error type calls retryable are tried again: a 404 or
    /// a malformed body returns on the first attempt, because waiting 750ms to
    /// receive the same 404 twice more helps nobody.
    async fn with_retries<T, F, Fut>(&self, attempt: F) -> Result<T, SleeperError>
    where
        F: Fn() -> Fut,
        Fut: std::future::Future<Output = Result<T, SleeperError>>,
    {
        let mut backoff = Duration::from_millis(250);
        let mut attempts = 0;
        loop {
            match attempt().await {
                Ok(value) => return Ok(value),
                Err(error) => {
                    attempts += 1;
                    if !error.retryable() || attempts == RETRIES {
                        return Err(error);
                    }
                }
            }
            tokio::time::sleep(backoff).await;
            backoff *= 2;
        }
    }

    /// A GET that parses in-task, for the small payloads.
    pub(crate) async fn get_json<T: serde::de::DeserializeOwned>(
        &self,
        url: &str,
    ) -> Result<T, SleeperError> {
        let url = crate::sleeper_host::route(url);
        self.with_retries(|| self.get_json_once(&url)).await
    }

    /// A GET that hands back the raw body, for payloads too big to parse on
    /// the runtime thread. Same retry policy as `get_json`.
    pub(crate) async fn get_bytes(&self, url: &str) -> Result<Vec<u8>, SleeperError> {
        let url = crate::sleeper_host::route(url);
        self.with_retries(|| self.get_bytes_once(&url)).await
    }

    /// Resolve a Sleeper username to its user id.
    ///
    /// Sleeper usernames are alphanumerics plus `_` and `-`; anything else is
    /// refused rather than escaped, because it would be interpolated into the
    /// request path.
    pub async fn user(&self, username: &str) -> Result<SleeperUser, SleeperError> {
        let username = username.trim();
        let legal = !username.is_empty()
            && username.len() <= 32
            && username
                .chars()
                .all(|c| c.is_ascii_alphanumeric() || c == '_' || c == '-');
        if !legal {
            return Err(SleeperError::Invalid(format!(
                "'{username}' is not a valid Sleeper username"
            )));
        }
        let user: Option<SleeperUser> = self.get_json(&format!("{BASE}/user/{username}")).await?;
        user.ok_or_else(|| SleeperError::NotFound(format!("Sleeper user '{username}' not found")))
    }

    pub async fn league(&self, league_id: &str) -> Result<League, SleeperError> {
        let v: Option<League> = self.get_json(&format!("{BASE}/league/{league_id}")).await?;
        v.ok_or_else(|| {
            SleeperError::NotFound(format!(
                "league {league_id} not found (Sleeper returned null)"
            ))
        })
    }

    pub async fn draft(&self, draft_id: &str) -> Result<Draft, SleeperError> {
        let v: Option<Draft> = self.get_json(&format!("{BASE}/draft/{draft_id}")).await?;
        v.ok_or_else(|| {
            SleeperError::NotFound(format!(
                "draft {draft_id} not found (Sleeper returned null)"
            ))
        })
    }

    pub async fn picks(&self, draft_id: &str) -> Result<Vec<Pick>, SleeperError> {
        let v: Option<Vec<Pick>> = self
            .get_json(&format!("{BASE}/draft/{draft_id}/picks"))
            .await?;
        Ok(v.unwrap_or_default())
    }

    /// All members of a league (for slot display names). One call.
    pub async fn league_users(&self, league_id: &str) -> Result<Vec<LeagueUser>, SleeperError> {
        let v: Option<Vec<LeagueUser>> = self
            .get_json(&format!("{BASE}/league/{league_id}/users"))
            .await?;
        Ok(v.unwrap_or_default())
    }

    /// Full player dictionary, unparsed: ~14.6 MB of JSON, cached on disk.
    ///
    /// Bytes rather than a `HashMap` because the caller parses it on the
    /// blocking pool — see `projections::players`.
    pub async fn players_bytes(&self) -> Result<Vec<u8>, SleeperError> {
        self.get_bytes(&format!("{BASE}/players/nfl")).await
    }

    /// Undocumented: full-season raw-stat projections for one season.
    pub async fn season_projections(
        &self,
        season: u32,
    ) -> Result<Vec<ProjectionRow>, SleeperError> {
        let url = format!(
            "{BASE_UNDOC}/projections/nfl/{season}?season_type=regular&position[]=QB&position[]=RB&position[]=WR&position[]=TE&position[]=K&position[]=DEF&order_by=adp_ppr"
        );
        self.get_json(&url).await
    }

    /// Undocumented: one week's raw-stat projections (for per-game bonus modeling).
    pub async fn weekly_projections(
        &self,
        season: u32,
        week: u32,
    ) -> Result<Vec<ProjectionRow>, SleeperError> {
        let url = format!(
            "{BASE_UNDOC}/projections/nfl/{season}/{week}?season_type=regular&position[]=QB&position[]=RB&position[]=WR&position[]=TE&position[]=K&position[]=DEF"
        );
        self.get_json(&url).await
    }
}
