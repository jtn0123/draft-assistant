//! Assembles everything: cached Sleeper data -> scored board -> draft view.
//!
//! One `DraftView` struct is BOTH the UI's data source and the AI-readable
//! state dump — there is deliberately no difference between what the human
//! and the model can see.

use crate::board::{build_board, BoardPlayer};
use crate::sleeper::{Draft, League, Pick, PlayerMeta, ProjectionRow, SleeperClient};
use serde::{Deserialize, Serialize};
use std::collections::HashMap;
use std::path::PathBuf;
use std::time::{SystemTime, UNIX_EPOCH};

const PLAYERS_TTL_SECS: u64 = 24 * 3600;
const PROJECTIONS_TTL_SECS: u64 = 6 * 3600;
const WEEKS: u32 = 18;

pub(crate) fn now_secs() -> u64 {
    SystemTime::now().duration_since(UNIX_EPOCH).map(|d| d.as_secs()).unwrap_or(0)
}

// ---------- persisted config ----------

#[derive(Debug, Clone, Serialize, Deserialize, Default)]
pub struct AppConfig {
    pub my_user_id: Option<String>,
    pub active_league_id: Option<String>,
    #[serde(default)]
    pub leagues: Vec<StoredLeague>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct StoredLeague {
    pub league_id: String,
    pub name: String,
    pub season: String,
}

// ---------- cache envelope ----------

#[derive(Serialize, Deserialize)]
struct Cached<T> {
    fetched_at: u64,
    data: T,
}

// ---------- engine ----------

pub struct LoadedLeague {
    pub league: League,
    pub draft: Draft,
    /// user_id -> display name, from /league/{id}/users.
    pub user_names: HashMap<String, String>,
    pub board: Vec<BoardPlayer>,
    pub board_index: HashMap<String, usize>,
    pub api_picks: Vec<Pick>,
    pub manual_picks: Vec<Pick>,
    pub players_fetched_at: u64,
    pub projections_fetched_at: u64,
    pub weekly_fetched_at: u64,
    pub warnings: Vec<String>,
    pub player_meta: HashMap<String, PlayerMeta>,
}

pub struct Engine {
    pub client: SleeperClient,
    pub data_dir: PathBuf,
}

impl Engine {
    pub fn new(data_dir: PathBuf) -> Self {
        std::fs::create_dir_all(&data_dir).ok();
        Self { client: SleeperClient::new(), data_dir }
    }

    fn cache_path(&self, name: &str) -> PathBuf {
        self.data_dir.join(name)
    }

    fn read_cache<T: serde::de::DeserializeOwned>(&self, name: &str, ttl: u64) -> Option<(u64, T)> {
        let raw = std::fs::read_to_string(self.cache_path(name)).ok()?;
        let cached: Cached<T> = serde_json::from_str(&raw).ok()?;
        if now_secs().saturating_sub(cached.fetched_at) > ttl {
            return None;
        }
        Some((cached.fetched_at, cached.data))
    }

    fn write_cache<T: Serialize>(&self, name: &str, data: &T) -> u64 {
        let fetched_at = now_secs();
        let env = Cached { fetched_at, data };
        if let Ok(json) = serde_json::to_string(&env) {
            let tmp = self.cache_path(&format!("{name}.tmp"));
            if std::fs::write(&tmp, json).is_ok() {
                std::fs::rename(tmp, self.cache_path(name)).ok();
            }
        }
        fetched_at
    }

    pub fn load_config(&self) -> AppConfig {
        std::fs::read_to_string(self.cache_path("config.json"))
            .ok()
            .and_then(|s| serde_json::from_str(&s).ok())
            .unwrap_or_default()
    }

    pub fn save_config(&self, config: &AppConfig) {
        if let Ok(json) = serde_json::to_string_pretty(config) {
            std::fs::write(self.cache_path("config.json"), json).ok();
        }
    }

    async fn players(&self, force: bool) -> Result<(u64, HashMap<String, PlayerMeta>), String> {
        if !force {
            if let Some(hit) = self.read_cache("players.json", PLAYERS_TTL_SECS) {
                return Ok(hit);
            }
        }
        let data = self.client.players().await?;
        let at = self.write_cache("players.json", &data);
        Ok((at, data))
    }

    async fn season_projections(
        &self,
        season: u32,
        force: bool,
    ) -> Result<(u64, Vec<ProjectionRow>), String> {
        let name = format!("projections_{season}.json");
        if !force {
            if let Some(hit) = self.read_cache(&name, PROJECTIONS_TTL_SECS) {
                return Ok(hit);
            }
        }
        let data = self.client.season_projections(season).await?;
        let at = self.write_cache(&name, &data);
        Ok((at, data))
    }

    async fn weekly_projections(
        &self,
        season: u32,
        force: bool,
    ) -> Result<(u64, Vec<ProjectionRow>), String> {
        let name = format!("weekly_{season}.json");
        if !force {
            if let Some(hit) = self.read_cache(&name, PROJECTIONS_TTL_SECS) {
                return Ok(hit);
            }
        }
        let mut all = Vec::new();
        for week in 1..=WEEKS {
            match self.client.weekly_projections(season, week).await {
                Ok(mut rows) => {
                    for r in &mut rows {
                        r.week = Some(week);
                    }
                    all.extend(rows);
                }
                Err(e) => {
                    // A missing week degrades bonus precision, not correctness.
                    eprintln!("weekly projections week {week} failed: {e}");
                }
            }
        }
        let at = self.write_cache(&name, &all);
        Ok((at, all))
    }

    /// Load a league end-to-end and build its scored board.
    pub async fn load_league(&self, league_id: &str, force: bool) -> Result<LoadedLeague, String> {
        let league = self.client.league(league_id).await?;
        let draft_id = league
            .draft_id
            .clone()
            .ok_or_else(|| "league has no draft".to_string())?;
        let draft = self.client.draft(&draft_id).await?;
        let user_names: HashMap<String, String> = self
            .client
            .league_users(league_id)
            .await
            .unwrap_or_default()
            .into_iter()
            .filter_map(|u| u.display_name.map(|n| (u.user_id, n)))
            .collect();
        self.assemble(league, draft, user_names, force).await
    }

    /// Load a bare draft ID (mock drafts have no league): synthesize the
    /// league settings from the draft's own settings + scoring_type.
    pub async fn load_draft_only(&self, draft_id: &str, force: bool) -> Result<LoadedLeague, String> {
        let draft = self.client.draft(draft_id).await?;
        let league = synthesize_league(&draft);
        let mut loaded = self.assemble(league, draft, HashMap::new(), force).await?;
        loaded
            .warnings
            .push("mock draft: league settings synthesized from draft settings".into());
        Ok(loaded)
    }

    /// Try the ID as a league first, then as a bare draft (mock).
    pub async fn load_any(&self, id: &str, force: bool) -> Result<LoadedLeague, String> {
        match self.load_league(id, force).await {
            Ok(l) => Ok(l),
            Err(league_err) => self
                .load_draft_only(id, force)
                .await
                .map_err(|draft_err| format!("not a league ({league_err}); not a draft ({draft_err})")),
        }
    }

    async fn assemble(
        &self,
        league: League,
        draft: Draft,
        user_names: HashMap<String, String>,
        force: bool,
    ) -> Result<LoadedLeague, String> {
        let api_picks = self.client.picks(&draft.draft_id).await.unwrap_or_default();
        let season: u32 = league.season.parse().map_err(|_| "bad season".to_string())?;
        let (players_at, player_meta) = self.players(force).await?;
        let (proj_at, season_rows) = self.season_projections(season, force).await?;
        let (weekly_at, weekly_rows) = self.weekly_projections(season, force).await?;

        let mut warnings = Vec::new();
        let board = build_board(
            &league,
            &draft,
            &player_meta,
            &season_rows,
            &weekly_rows,
            &mut warnings,
        );
        if board.len() < 200 {
            warnings.push(format!(
                "board unusually small ({} players) — projections may be incomplete",
                board.len()
            ));
        }
        let board_index = board
            .iter()
            .enumerate()
            .map(|(i, p)| (p.player_id.clone(), i))
            .collect();

        Ok(LoadedLeague {
            league,
            draft,
            user_names,
            board,
            board_index,
            api_picks,
            manual_picks: Vec::new(),
            players_fetched_at: players_at,
            projections_fetched_at: proj_at,
            weekly_fetched_at: weekly_at,
            warnings,
            player_meta,
        })
    }
}


/// Build a stand-in League for a leagueless (mock) draft: roster shape from
/// the draft's slot counts, scoring from Sleeper's default std/half/full PPR.
fn synthesize_league(draft: &Draft) -> League {
    let s = &draft.settings;
    let mut roster_positions: Vec<String> = Vec::new();
    let mut push = |pos: &str, n: Option<u32>| {
        for _ in 0..n.unwrap_or(0) {
            roster_positions.push(pos.to_string());
        }
    };
    push("QB", s.slots_qb);
    push("RB", s.slots_rb);
    push("WR", s.slots_wr);
    push("TE", s.slots_te);
    push("FLEX", s.slots_flex);
    push("SUPER_FLEX", s.slots_super_flex);
    push("K", s.slots_k);
    push("DEF", s.slots_def);
    let starters = roster_positions.len() as u32;
    for _ in 0..s.rounds.saturating_sub(starters) {
        roster_positions.push("BN".to_string());
    }

    let scoring_type = draft
        .metadata
        .as_ref()
        .and_then(|m| m.scoring_type.clone())
        .unwrap_or_else(|| "ppr".into());
    let ppr = match scoring_type.as_str() {
        "ppr" => 1.0,
        "half_ppr" => 0.5,
        _ => 0.0,
    };
    let scoring_settings: HashMap<String, f64> = [
        ("pass_yd", 0.04), ("pass_td", 4.0), ("pass_int", -1.0), ("pass_2pt", 2.0),
        ("rush_yd", 0.1), ("rush_td", 6.0), ("rush_2pt", 2.0),
        ("rec_yd", 0.1), ("rec_td", 6.0), ("rec_2pt", 2.0), ("rec", ppr),
        ("fum_lost", -2.0),
        ("sack", 1.0), ("int", 2.0), ("fum_rec", 2.0), ("def_td", 6.0),
        ("safe", 2.0), ("blk_kick", 2.0), ("def_st_td", 6.0),
        ("pts_allow_0", 10.0), ("pts_allow_1_6", 7.0), ("pts_allow_7_13", 4.0),
        ("pts_allow_14_20", 1.0), ("pts_allow_21_27", 0.0),
        ("pts_allow_28_34", -1.0), ("pts_allow_35p", -4.0),
    ]
    .into_iter()
    .map(|(k, v)| (k.to_string(), v))
    .collect();

    let name = draft
        .metadata
        .as_ref()
        .and_then(|m| m.name.clone())
        .filter(|n| !n.is_empty())
        .unwrap_or_else(|| format!("Mock draft ({scoring_type})"));
    League {
        league_id: draft.draft_id.clone(),
        name,
        season: draft.season.clone().unwrap_or_else(|| "2026".into()),
        status: draft.status.clone(),
        total_rosters: s.teams,
        roster_positions,
        scoring_settings,
        draft_id: Some(draft.draft_id.clone()),
    }
}

pub use crate::view::{build_view, merged_picks, DraftView};
