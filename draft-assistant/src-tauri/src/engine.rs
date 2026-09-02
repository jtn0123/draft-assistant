//! Assembles everything: cached Sleeper data -> scored board -> draft view.
//!
//! One `DraftView` struct is BOTH the UI's data source and the AI-readable
//! state dump — there is deliberately no difference between what the human
//! and the model can see.

use crate::board::{build_board, BoardPlayer};
use crate::cache::{envelope_json, fresh_enough, read_cached, replace_file, write_atomic};
use crate::keepers::KeeperStore;
use crate::mock_league::synthesize_league;
use crate::picks::{reconcile_manual_picks, ManualPickStore};
use crate::roster::RosterRules;
use crate::sleeper::{Draft, League, Pick, PlayerMeta, SleeperClient};
use crate::sleeper_error::to_message;
use crate::traded_picks::TradedPick;
use crate::valuation::ReplacementModel;
use crate::weekly::WeeklyPoints;
use serde::{Deserialize, Serialize};
use std::collections::{HashMap, HashSet};
use std::path::PathBuf;
use std::time::{SystemTime, UNIX_EPOCH};

pub(crate) const PLAYERS_TTL_SECS: u64 = 24 * 3600;
pub(crate) const PROJECTIONS_TTL_SECS: u64 = 6 * 3600;
pub(crate) const WEEKS: u32 = 18;
/// How many Sleeper requests to have in flight at once. Enough to hide the
/// round trips, well short of anything that looks like hammering.
pub(crate) const REQUEST_CONCURRENCY: usize = 6;

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
    /// Key for the Ask Claude panel. Stored in the app's own data directory
    /// and never sent anywhere except api.anthropic.com.
    #[serde(default)]
    pub anthropic_api_key: Option<String>,
    /// How Ask Claude reaches Claude: "api" (the key above) or "claude_code"
    /// (the Claude Code CLI, signed in with a subscription). Unset means
    /// whichever is available, preferring the CLI when there is no key.
    #[serde(default)]
    pub chat_provider: Option<String>,
    /// Dollars one screen's Ask Claude may spend before the backend refuses
    /// the next turn. `None` means nobody has set one and the default is in
    /// force; `Some(0.0)` means the user turned the cap off.
    #[serde(default)]
    pub chat_budget_usd: Option<f64>,
    /// screen ("draft" / "season") -> what that screen's chats have cost, all
    /// conversations together. The cap is checked against this, so it has to
    /// outlive both the conversation and the app.
    #[serde(default)]
    pub chat_spend_usd: HashMap<String, f64>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct StoredLeague {
    pub league_id: String,
    pub name: String,
    pub season: String,
}

// ---------- engine ----------

#[derive(Clone)]
pub struct LoadedLeague {
    pub league: League,
    pub draft: Draft,
    /// user_id -> display name, from /league/{id}/users.
    pub user_names: HashMap<String, String>,
    /// user_id -> avatar hash or custom image URL, same call.
    pub user_avatars: HashMap<String, String>,
    pub board: Vec<BoardPlayer>,
    pub board_index: HashMap<String, usize>,
    pub replacement_model: ReplacementModel,
    pub roster_rules: RosterRules,
    pub api_picks: Vec<Pick>,
    pub manual_picks: Vec<Pick>,
    /// Draft picks that changed hands, from `/draft/{id}/traded_picks`. Empty
    /// in a league that trades none, and whenever the fetch fails — in which
    /// case pick ownership falls back to the plain snake.
    pub traded_picks: Vec<TradedPick>,
    /// Pick numbers known to be keepers: flagged by Sleeper, or seen sitting
    /// ahead of the clock at some point. Remembered on disk because a keeper
    /// stays a keeper once the draft passes its slot.
    pub keeper_pick_nos: HashSet<u32>,
    pub poll_last_success_at: Option<u64>,
    pub poll_consecutive_failures: u32,
    pub poll_last_error: Option<String>,
    pub players_fetched_at: u64,
    pub projections_fetched_at: u64,
    pub weekly_fetched_at: u64,
    pub warnings: Vec<String>,
    pub player_meta: HashMap<String, PlayerMeta>,
    /// Per-week projected points under this league's scoring. Built once here
    /// so the season screen never re-scores raw stat lines.
    pub weekly_points: WeeklyPoints,
    /// When the imported second-opinion CSV was read, epoch seconds. `None`
    /// when there is none to read.
    pub second_opinion_loaded_at: Option<u64>,
}

pub struct Engine {
    pub client: SleeperClient,
    pub data_dir: PathBuf,
    /// The Keychain's answer, remembered.
    ///
    /// Reading it shells out to `/usr/bin/security`: tens of milliseconds on a
    /// good day and unbounded if the Keychain decides to prompt. The chat
    /// panel asks on every question and on every settings render, so the
    /// answer is fetched once and kept until the key is changed. `None` means
    /// "not looked up yet"; `Some(None)` means "looked and there is none".
    pub(crate) key_cache: tokio::sync::Mutex<Option<Option<String>>>,
}

/// The one thing that talks to Sleeper and owns the on-disk cache.
///
/// Its draft-loading surface is below. Everything else `Engine` can do is
/// declared as a trait next to the code that implements it, so this list is
/// the whole story:
///
/// - [`crate::projections`] — the players dictionary and projection fetches
/// - [`crate::headshots::ImageCache`] — player photos and manager avatars
/// - [`crate::season_engine::SeasonLoader`] — loading and refreshing a season
/// - [`crate::season_history::HistoryStore`] — Trends snapshots
/// - [`crate::picks::ManualPickStore`] — picks the user typed in by hand
/// - [`crate::keepers::KeeperStore`] — which picks this league keeps
impl Engine {
    pub fn new(data_dir: PathBuf) -> Self {
        std::fs::create_dir_all(&data_dir).ok();
        // Everything under here — rosters, league member names, Sleeper user
        // ids, the players dictionary — is the user's alone to read.
        crate::cache::owner_only_dir(&data_dir);
        Self {
            client: SleeperClient::new(),
            data_dir,
            key_cache: tokio::sync::Mutex::new(None),
        }
    }

    fn cache_path(&self, name: &str) -> PathBuf {
        self.data_dir.join(name)
    }

    pub(crate) fn read_cache<T: serde::de::DeserializeOwned>(
        &self,
        name: &str,
        ttl: u64,
    ) -> Option<(u64, T)> {
        fresh_enough(self.read_cache_any(name), ttl)
    }

    pub(crate) fn read_cache_any<T: serde::de::DeserializeOwned>(
        &self,
        name: &str,
    ) -> Option<(u64, T)> {
        read_cached(self.cache_path(name))
    }

    /// `read_cache_any` off the async runtime.
    ///
    /// The players dictionary is ~15 MB of JSON; parsing it on the runtime
    /// thread stalls every other task, including the poll loop, for as long
    /// as it takes.
    pub(crate) async fn read_cache_any_off_thread<T>(&self, name: &str) -> Option<(u64, T)>
    where
        T: serde::de::DeserializeOwned + Send + 'static,
    {
        let path = self.cache_path(name);
        tokio::task::spawn_blocking(move || read_cached(path))
            .await
            .ok()?
    }

    /// `read_cache` off the async runtime. See `read_cache_any_off_thread`.
    pub(crate) async fn read_cache_off_thread<T>(&self, name: &str, ttl: u64) -> Option<(u64, T)>
    where
        T: serde::de::DeserializeOwned + Send + 'static,
    {
        fresh_enough(self.read_cache_any_off_thread(name).await, ttl)
    }

    /// `write_cache` off the async runtime. Takes the value by reference and
    /// serializes it here, then hands only the finished bytes to the blocking
    /// pool, so callers keep ownership of what they just fetched.
    pub(crate) async fn write_cache_off_thread<T: Serialize>(&self, name: &str, data: &T) -> u64 {
        let fetched_at = now_secs();
        let Ok(json) = envelope_json(fetched_at, data) else {
            return fetched_at;
        };
        let tmp = self.cache_path(&format!("{name}.tmp"));
        let final_path = self.cache_path(name);
        let _ = tokio::task::spawn_blocking(move || replace_file(tmp, final_path, json)).await;
        fetched_at
    }

    pub(crate) fn write_cache<T: Serialize>(&self, name: &str, data: &T) -> u64 {
        let fetched_at = now_secs();
        let tmp = self.cache_path(&format!("{name}.tmp"));
        write_atomic(tmp, self.cache_path(name), fetched_at, data).ok();
        fetched_at
    }

    pub(crate) fn write_cache_checked<T: Serialize>(
        &self,
        name: &str,
        data: &T,
    ) -> Result<u64, String> {
        let fetched_at = now_secs();
        let tmp = self.cache_path(&format!("{name}.tmp"));
        write_atomic(tmp, self.cache_path(name), fetched_at, data)
            .map_err(|e| format!("{name}: {e}"))?;
        Ok(fetched_at)
    }

    /// Read the config, falling back to the last good copy if the live file
    /// is missing or unreadable. A key still sitting in the file from before
    /// Keychain storage existed is moved there on the way in.
    pub fn load_config(&self) -> AppConfig {
        let read = |name: &str| {
            std::fs::read_to_string(self.cache_path(name))
                .ok()
                .and_then(|s| serde_json::from_str::<AppConfig>(&s).ok())
        };
        let mut config = read("config.json")
            .or_else(|| read("config.json.bak"))
            .unwrap_or_default();
        if let Some(key) = config.anthropic_api_key.take() {
            if crate::secrets::available() && crate::secrets::store(&key).is_ok() {
                // The key is safely in the Keychain either way; if rewriting
                // the file to drop it fails, the next save tries again.
                let _ = self.save_config(&config);
            } else {
                config.anthropic_api_key = Some(key);
            }
        }
        config
    }

    /// Write the config atomically: to a temp file first, then swapped into
    /// place, with the previous copy kept as `config.json.bak`. A crash
    /// mid-write can never leave a half-written config behind.
    ///
    /// Every failure comes back to the caller: a save that quietly did nothing
    /// loses the user's league list at the next launch with nothing said.
    pub fn save_config(&self, config: &AppConfig) -> Result<(), String> {
        let json = serde_json::to_string_pretty(config)
            .map_err(|e| format!("could not prepare your settings to be saved: {e}"))?;
        let live = self.cache_path("config.json");
        let tmp = self.cache_path("config.json.tmp");
        std::fs::write(&tmp, json)
            .map_err(|e| format!("could not save your settings to {}: {e}", tmp.display()))?;
        crate::cache::owner_only(&tmp);
        if live.exists() {
            std::fs::copy(&live, self.cache_path("config.json.bak")).ok();
        }
        std::fs::rename(&tmp, &live)
            .map_err(|e| format!("could not save your settings to {}: {e}", live.display()))
    }

    /// Load a league end-to-end and build its scored board.
    pub async fn load_league(&self, league_id: &str, force: bool) -> Result<LoadedLeague, String> {
        let league = self.client.league(league_id).await.map_err(to_message)?;
        let draft_id = league
            .draft_id
            .clone()
            .ok_or_else(|| "league has no draft".to_string())?;
        // The draft and the member list depend on nothing but ids we already
        // have, so they go out together rather than one waiting on the other.
        let (draft, users) = tokio::join!(
            self.client.draft(&draft_id),
            self.client.league_users(league_id)
        );
        let draft = draft.map_err(to_message)?;
        let users = users.unwrap_or_default();
        let user_names = crate::sleeper::label_map(&users);
        let user_avatars = crate::sleeper::avatar_map(&users);
        self.assemble(league, draft, user_names, user_avatars, force)
            .await
    }

    /// Load a bare draft ID (mock drafts have no league): synthesize the
    /// league settings from the draft's own settings + scoring_type.
    pub async fn load_draft_only(
        &self,
        draft_id: &str,
        force: bool,
    ) -> Result<LoadedLeague, String> {
        let draft = self.client.draft(draft_id).await.map_err(to_message)?;
        let league = synthesize_league(&draft);
        let mut loaded = self
            .assemble(league, draft, HashMap::new(), HashMap::new(), force)
            .await?;
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
        user_avatars: HashMap<String, String>,
        force: bool,
    ) -> Result<LoadedLeague, String> {
        // The picks and the trade list are independent reads of the same
        // draft, so they go out together.
        let (picks, traded) = tokio::join!(
            self.client.picks(&draft.draft_id),
            self.client.traded_picks(&draft.draft_id)
        );
        let (api_picks, poll_last_success_at, poll_consecutive_failures, poll_last_error) =
            match picks.map_err(to_message) {
                Ok(picks) => (picks, Some(now_secs()), 0, None),
                Err(error) => (Vec::new(), None, 1, Some(error)),
            };
        // A missing trade list is not worth failing a load over: pick
        // ownership simply falls back to the plain snake.
        let (traded_picks, traded_warning) = match traded.map_err(to_message) {
            Ok(traded) => (traded, None),
            Err(error) => (
                Vec::new(),
                Some(format!(
                    "traded draft picks unavailable ({error}); pick order shown as a plain snake"
                )),
            ),
        };
        let mut manual_picks = self.load_manual_picks(&draft.draft_id);
        if reconcile_manual_picks(&api_picks, &mut manual_picks) {
            self.save_manual_picks(&draft.draft_id, &manual_picks)?;
        }
        // Every pick calculation divides by the team count and counts up to
        // teams * rounds, so a draft that reports neither is refused here
        // rather than panicking on the next view build.
        if draft.settings.teams == 0 || draft.settings.rounds == 0 {
            return Err(format!(
                "draft {} reports {} teams and {} rounds — it has not been set up yet",
                draft.draft_id, draft.settings.teams, draft.settings.rounds
            ));
        }
        let season: u32 = league
            .season
            .parse()
            .map_err(|_| "bad season".to_string())?;
        // Three independent fetches — the third an eighteen-request fan-out of
        // its own. Run one after another at an eight-second timeout each, a
        // cold load served three rounds of latency where one would do.
        let (players, season_projections, weekly) = tokio::try_join!(
            self.players(force),
            self.season_projections(season, force),
            self.weekly_projections(season, force),
        )?;
        let (players_at, player_meta, players_warning) = players;
        let (proj_at, season_rows, projections_warning) = season_projections;
        let (weekly_at, weekly_rows, weekly_warning) = weekly;

        let mut warnings = Vec::new();
        warnings.extend(players_warning);
        warnings.extend(projections_warning);
        warnings.extend(weekly_warning);
        warnings.extend(traded_warning);
        if let Some(error) = &poll_last_error {
            warnings.push(format!("initial picks refresh failed: {error}"));
        }
        let scoring_map = league.scoring_settings.clone();
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
        let mut board = board_build.players;
        // The imported second opinion, if the user has ever chosen one. A file
        // that has stopped parsing becomes a warning rather than a failed
        // load: it is a nice-to-have column, not the board.
        let second_opinion_loaded_at = match crate::second_opinion::load(&self.data_dir) {
            Ok(Some(table)) => {
                let report = crate::second_opinion::apply(&table, &mut board);
                if report.matched == 0 {
                    warnings.push(
                        "imported projections matched nobody on this board — \
                         check it is the right season"
                            .into(),
                    );
                }
                Some(table.loaded_at)
            }
            Ok(None) => None,
            Err(error) => {
                warnings.push(format!("imported projections could not be read: {error}"));
                None
            }
        };
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

        let keeper_pick_nos = self.load_keepers(&draft.draft_id);
        Ok(LoadedLeague {
            league,
            draft,
            user_names,
            user_avatars,
            board,
            board_index,
            replacement_model: board_build.replacement,
            roster_rules,
            api_picks,
            manual_picks,
            traded_picks,
            keeper_pick_nos,
            poll_last_success_at,
            poll_consecutive_failures,
            poll_last_error,
            players_fetched_at: players_at,
            projections_fetched_at: proj_at,
            weekly_fetched_at: weekly_at,
            warnings,
            weekly_points: WeeklyPoints::build(&weekly_rows, &scoring_map),
            player_meta,
            second_opinion_loaded_at,
        })
    }
}

#[cfg(test)]
mod reliability_tests {
    use super::*;
    use crate::cache::Cached;

    fn test_dir(label: &str) -> PathBuf {
        let unique = format!(
            "draft-assistant-{label}-{}-{}",
            std::process::id(),
            now_secs()
        );
        std::env::temp_dir().join(unique)
    }

    #[test]
    fn expired_cache_is_still_available_for_outage_fallback() {
        let dir = test_dir("stale-cache");
        let engine = Engine::new(dir.clone());
        let cached = Cached {
            fetched_at: 1,
            data: vec![10_u32, 20_u32],
        };
        std::fs::write(
            engine.cache_path("test.json"),
            serde_json::to_string(&cached).unwrap(),
        )
        .unwrap();

        assert!(engine.read_cache::<Vec<u32>>("test.json", 1).is_none());
        assert_eq!(
            engine.read_cache_any::<Vec<u32>>("test.json"),
            Some((1, vec![10, 20]))
        );

        std::fs::remove_dir_all(dir).unwrap();
    }
}
