//! Assembles everything: cached Sleeper data -> scored board -> draft view.
//!
//! One `DraftView` struct is BOTH the UI's data source and the AI-readable
//! state dump — there is deliberately no difference between what the human
//! and the model can see.

use crate::board::{build_board, BoardPlayer};
use crate::mock_league::synthesize_league;
use crate::roster::RosterRules;
use crate::sleeper::{Draft, League, Pick, PlayerMeta, ProjectionRow, SleeperClient};
use crate::valuation::ReplacementModel;
use serde::{Deserialize, Serialize};
use std::collections::{HashMap, HashSet};
use std::path::PathBuf;
use std::time::{SystemTime, UNIX_EPOCH};

const PLAYERS_TTL_SECS: u64 = 24 * 3600;
const PROJECTIONS_TTL_SECS: u64 = 6 * 3600;
const WEEKS: u32 = 18;

pub(crate) fn now_secs() -> u64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|d| d.as_secs())
        .unwrap_or(0)
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
pub(crate) struct Cached<T> {
    pub fetched_at: u64,
    pub data: T,
}

// ---------- engine ----------

pub struct LoadedLeague {
    pub league: League,
    pub draft: Draft,
    /// user_id -> display name, from /league/{id}/users.
    pub user_names: HashMap<String, String>,
    pub board: Vec<BoardPlayer>,
    pub board_index: HashMap<String, usize>,
    pub replacement_model: ReplacementModel,
    pub roster_rules: RosterRules,
    pub api_picks: Vec<Pick>,
    pub manual_picks: Vec<Pick>,
    /// Pick numbers known to be keepers: flagged by Sleeper, or sitting in
    /// the book ahead of the draft's progress when first seen. Remembered so
    /// a keeper stays a keeper once the draft passes its slot — the flag
    /// alone cannot be trusted (`Pick::is_keeper`).
    pub keeper_pick_nos: HashSet<u32>,
    pub poll_last_success_at: Option<u64>,
    pub poll_consecutive_failures: u32,
    pub poll_last_error: Option<String>,
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
    /// Fails if the data directory cannot be created — without it nothing can
    /// be cached or saved, so it is better to say so at startup than to report
    /// success on every later write.
    pub fn new(data_dir: PathBuf) -> Result<Self, String> {
        std::fs::create_dir_all(&data_dir)
            .map_err(|e| format!("create data dir {}: {e}", data_dir.display()))?;
        Ok(Self {
            client: SleeperClient::new(),
            data_dir,
        })
    }

    async fn players(
        &self,
        force: bool,
    ) -> Result<(u64, HashMap<String, PlayerMeta>, Option<String>), String> {
        if !force {
            if let Some(hit) = self.read_cache("players.json", PLAYERS_TTL_SECS) {
                return Ok((hit.0, hit.1, None));
            }
        }
        let stale = self.read_cache_any("players.json");
        match self.client.players().await {
            Ok(data) => {
                let (at, warning) = self.write_cache("players.json", &data);
                Ok((at, data, warning))
            }
            Err(error) => stale
                .map(|(at, data)| {
                    let age = now_secs().saturating_sub(at);
                    (
                        at,
                        data,
                        Some(format!(
                            "players refresh failed; using cache aged {}h ({error})",
                            age / 3600
                        )),
                    )
                })
                .ok_or(error),
        }
    }

    async fn season_projections(
        &self,
        season: u32,
        force: bool,
    ) -> Result<(u64, Vec<ProjectionRow>, Option<String>), String> {
        let name = format!("projections_{season}.json");
        if !force {
            if let Some(hit) = self.read_cache(&name, PROJECTIONS_TTL_SECS) {
                return Ok((hit.0, hit.1, None));
            }
        }
        let stale = self.read_cache_any(&name);
        match self.client.season_projections(season).await {
            Ok(data) => {
                let (at, warning) = self.write_cache(&name, &data);
                Ok((at, data, warning))
            }
            Err(error) => stale
                .map(|(at, data)| {
                    let age = now_secs().saturating_sub(at);
                    (
                        at,
                        data,
                        Some(format!(
                            "projections refresh failed; using cache aged {}h ({error})",
                            age / 3600
                        )),
                    )
                })
                .ok_or(error),
        }
    }

    async fn weekly_projections(
        &self,
        season: u32,
        force: bool,
    ) -> Result<(u64, Vec<ProjectionRow>, Option<String>), String> {
        let name = format!("weekly_{season}.json");
        if !force {
            if let Some(hit) = self.read_cache(&name, PROJECTIONS_TTL_SECS) {
                return Ok((hit.0, hit.1, None));
            }
        }
        let stale = self.read_cache_any(&name);
        let mut all = Vec::new();
        let mut failures = Vec::new();
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
                    failures.push(week);
                }
            }
        }
        if failures.len() == WEEKS as usize {
            let error = "all weekly projection requests failed".to_string();
            return stale
                .map(|(at, data)| {
                    let age = now_secs().saturating_sub(at);
                    (
                        at,
                        data,
                        Some(format!(
                            "weekly projections refresh failed; using cache aged {}h",
                            age / 3600
                        )),
                    )
                })
                .ok_or(error);
        }
        let (at, cache_warning) = self.write_cache(&name, &all);
        let missing_warning = if failures.is_empty() {
            None
        } else {
            Some(format!(
                "weekly projections unavailable for weeks {}",
                failures
                    .iter()
                    .map(u32::to_string)
                    .collect::<Vec<_>>()
                    .join(", ")
            ))
        };
        // Both can happen at once; keep whichever fired, joined.
        let warning = match (missing_warning, cache_warning) {
            (Some(a), Some(b)) => Some(format!("{a}; {b}")),
            (a, b) => a.or(b),
        };
        Ok((at, all, warning))
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
    pub async fn load_draft_only(
        &self,
        draft_id: &str,
        force: bool,
    ) -> Result<LoadedLeague, String> {
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
            Err(league_err) => self.load_draft_only(id, force).await.map_err(|draft_err| {
                format!("not a league ({league_err}); not a draft ({draft_err})")
            }),
        }
    }

    async fn assemble(
        &self,
        league: League,
        draft: Draft,
        user_names: HashMap<String, String>,
        force: bool,
    ) -> Result<LoadedLeague, String> {
        let (api_picks, poll_last_success_at, poll_consecutive_failures, poll_last_error) =
            match self.client.picks(&draft.draft_id).await {
                Ok(picks) => (picks, Some(now_secs()), 0, None),
                Err(error) => (Vec::new(), None, 1, Some(error)),
            };
        let mut manual_picks = self.load_manual_picks(&draft.draft_id);
        if reconcile_manual_picks(&api_picks, &mut manual_picks) {
            self.save_manual_picks(&draft.draft_id, &manual_picks)?;
        }
        let season: u32 = league
            .season
            .parse()
            .map_err(|_| "bad season".to_string())?;
        let (players_at, player_meta, players_warning) = self.players(force).await?;
        let (proj_at, season_rows, projections_warning) =
            self.season_projections(season, force).await?;
        let (weekly_at, weekly_rows, weekly_warning) =
            self.weekly_projections(season, force).await?;

        let mut warnings = Vec::new();
        warnings.extend(players_warning);
        warnings.extend(projections_warning);
        warnings.extend(weekly_warning);
        if let Some(error) = &poll_last_error {
            warnings.push(format!("initial picks refresh failed: {error}"));
        }
        let roster_rules = RosterRules::new(&league.roster_positions);
        let board_build = build_board(
            &league,
            &draft,
            &player_meta,
            &season_rows,
            &weekly_rows,
            &roster_rules,
            &mut warnings,
        );
        let board = board_build.players;
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

        let keeper_pick_nos = crate::view::keeper_pick_nos(
            &api_picks,
            draft.settings.teams.max(1),
            draft.settings.rounds.max(1),
        );
        Ok(LoadedLeague {
            league,
            draft,
            user_names,
            board,
            board_index,
            replacement_model: board_build.replacement,
            roster_rules,
            api_picks,
            manual_picks,
            keeper_pick_nos,
            poll_last_success_at,
            poll_consecutive_failures,
            poll_last_error,
            players_fetched_at: players_at,
            projections_fetched_at: proj_at,
            weekly_fetched_at: weekly_at,
            warnings,
            player_meta,
        })
    }
}

pub(crate) fn reconcile_manual_picks(api: &[Pick], manual: &mut Vec<Pick>) -> bool {
    let before = manual.len();
    let api_max = api.iter().map(|pick| pick.pick_no).max().unwrap_or(0);
    let api_players: std::collections::HashSet<&str> =
        api.iter().map(|pick| pick.player_id.as_str()).collect();
    manual.retain(|pick| pick.pick_no > api_max && !api_players.contains(pick.player_id.as_str()));
    manual.len() != before
}

#[cfg(test)]
mod reliability_tests {
    use super::*;
    use crate::sleeper::Pick;

    fn test_dir(label: &str) -> PathBuf {
        let unique = format!(
            "draft-assistant-{label}-{}-{}",
            std::process::id(),
            now_secs()
        );
        std::env::temp_dir().join(unique)
    }

    fn pick(pick_no: u32, player_id: &str) -> Pick {
        Pick {
            round: 1,
            pick_no,
            draft_slot: pick_no,
            player_id: player_id.into(),
            picked_by: None,
            metadata: None,
            is_keeper: None,
        }
    }

    #[test]
    fn engine_new_reports_an_unusable_data_dir() {
        let dir = test_dir("engine-new-file");
        std::fs::create_dir_all(dir.parent().unwrap()).ok();
        // A regular file where the data dir should be: create_dir_all must fail.
        std::fs::write(&dir, b"not a directory").unwrap();
        let err = match Engine::new(dir.clone()) {
            Ok(_) => panic!("file in the way must error"),
            Err(e) => e,
        };
        assert!(err.contains("create data dir"), "{err}");
        std::fs::remove_file(dir).unwrap();
    }

    #[test]
    fn manual_picks_survive_reload_and_reconcile_with_api_progress() {
        let dir = test_dir("manual-picks");
        let engine = Engine::new(dir.clone()).expect("temp data dir");
        let manual = vec![pick(1, "manual-1"), pick(2, "manual-2")];

        engine.save_manual_picks("draft-123", &manual).unwrap();
        let mut reloaded = engine.load_manual_picks("draft-123");
        assert_eq!(reloaded.len(), 2);

        let api = vec![pick(1, "api-1")];
        assert!(reconcile_manual_picks(&api, &mut reloaded));
        assert_eq!(reloaded.len(), 1);
        assert_eq!(reloaded[0].pick_no, 2);

        std::fs::remove_dir_all(dir).unwrap();
    }
}
