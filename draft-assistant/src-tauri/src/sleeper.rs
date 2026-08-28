//! Read-only Sleeper API client.
//!
//! Everything here is unauthenticated GETs against api.sleeper.app.
//! The projections endpoints are undocumented, so every response is
//! deserialized defensively (unknown fields ignored, missing fields defaulted)
//! and raw JSON snapshots are cached on disk by the caller.

use serde::{Deserialize, Serialize};
use std::collections::HashMap;

const BASE: &str = "https://api.sleeper.app/v1";
const BASE_UNDOC: &str = "https://api.sleeper.app";

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
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct DraftSettings {
    pub teams: u32,
    pub rounds: u32,
    #[serde(default)]
    pub pick_timer: Option<u32>,
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

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct LeagueUser {
    pub user_id: String,
    #[serde(default)]
    pub display_name: Option<String>,
}

pub struct SleeperClient {
    http: reqwest::Client,
}

impl Default for SleeperClient {
    fn default() -> Self {
        Self::new()
    }
}

impl SleeperClient {
    pub fn new() -> Self {
        let http = reqwest::Client::builder()
            .user_agent("draft-assistant/0.1 (local second-screen tool)")
            .gzip(true)
            .build()
            .expect("failed to build http client");
        Self { http }
    }

    async fn get_json<T: serde::de::DeserializeOwned>(&self, url: &str) -> Result<T, String> {
        let resp = self
            .http
            .get(url)
            .send()
            .await
            .map_err(|e| format!("request failed: {url}: {e}"))?;
        if !resp.status().is_success() {
            return Err(format!("HTTP {} for {url}", resp.status()));
        }
        resp.json::<T>()
            .await
            .map_err(|e| format!("bad JSON from {url}: {e}"))
    }

    pub async fn league(&self, league_id: &str) -> Result<League, String> {
        let v: Option<League> = self.get_json(&format!("{BASE}/league/{league_id}")).await?;
        v.ok_or_else(|| format!("league {league_id} not found (Sleeper returned null)"))
    }

    pub async fn draft(&self, draft_id: &str) -> Result<Draft, String> {
        let v: Option<Draft> = self.get_json(&format!("{BASE}/draft/{draft_id}")).await?;
        v.ok_or_else(|| format!("draft {draft_id} not found (Sleeper returned null)"))
    }

    pub async fn picks(&self, draft_id: &str) -> Result<Vec<Pick>, String> {
        let v: Option<Vec<Pick>> = self
            .get_json(&format!("{BASE}/draft/{draft_id}/picks"))
            .await?;
        Ok(v.unwrap_or_default())
    }

    /// All members of a league (for slot display names). One call.
    pub async fn league_users(&self, league_id: &str) -> Result<Vec<LeagueUser>, String> {
        let v: Option<Vec<LeagueUser>> = self
            .get_json(&format!("{BASE}/league/{league_id}/users"))
            .await?;
        Ok(v.unwrap_or_default())
    }

    /// Full player dictionary: player_id -> meta. ~14.6MB, cache on disk.
    pub async fn players(&self) -> Result<HashMap<String, PlayerMeta>, String> {
        self.get_json(&format!("{BASE}/players/nfl")).await
    }

    /// Undocumented: full-season raw-stat projections for one season.
    pub async fn season_projections(&self, season: u32) -> Result<Vec<ProjectionRow>, String> {
        let url = format!(
            "{BASE_UNDOC}/projections/nfl/{season}?season_type=regular&position[]=QB&position[]=RB&position[]=WR&position[]=TE&position[]=DEF&order_by=adp_ppr"
        );
        self.get_json(&url).await
    }

    /// Undocumented: one week's raw-stat projections (for per-game bonus modeling).
    pub async fn weekly_projections(
        &self,
        season: u32,
        week: u32,
    ) -> Result<Vec<ProjectionRow>, String> {
        let url = format!(
            "{BASE_UNDOC}/projections/nfl/{season}/{week}?season_type=regular&position[]=QB&position[]=RB&position[]=WR&position[]=TE&position[]=DEF"
        );
        self.get_json(&url).await
    }
}
