//! Assembles everything: cached Sleeper data -> scored board -> draft view.
//!
//! One `DraftView` struct is BOTH the UI's data source and the AI-readable
//! state dump — there is deliberately no difference between what the human
//! and the model can see.

use crate::board::BoardPlayer;
use crate::cache::{
    envelope_json, fresh_enough, read_cached, replace_file, temp_sibling, write_atomic,
};
use crate::engine_assemble::AssemblyParts;
use crate::mock_league::synthesize_league;
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
    /// What this Mac calls itself in the shared chat and on a follower's
    /// "Hosted by …" pill. Unset until the user edits it, and then the
    /// machine's own computer name is used.
    #[serde(default)]
    pub device_name: Option<String>,
    /// The port the phone / second-screen server last listened on, so the URL
    /// a user bookmarked keeps working across restarts. Unset means the
    /// default, 7878.
    #[serde(default)]
    pub companion_port: Option<u16>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct StoredLeague {
    pub league_id: String,
    pub name: String,
    pub season: String,
    /// Sleeper's `pre_draft`/`drafting`/`in_season`/`complete`; absent for
    /// older configs and for a mock draft, which has no league to ask.
    #[serde(default)]
    pub status: Option<String>,
    /// `"sleeper"` or `"yahoo"`. Defaulted so a config written before Yahoo
    /// existed still loads, with every league in it read as a Sleeper one —
    /// which is what it was.
    #[serde(default = "sleeper")]
    pub platform: String,
}

/// The platform a stored league has when its config predates the field.
fn sleeper() -> String {
    crate::view_types::SLEEPER.to_string()
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
    /// The slot the logged-in user's own team drafts from, when the platform
    /// says so outright. Yahoo flags the current login on a team's manager;
    /// Sleeper has no such flag and leaves this `None`, resolving "my team"
    /// through the Sleeper user id in the config instead.
    pub my_slot: Option<u32>,
    /// `yahoo:<player id>` -> the id that player sits on the board under, from
    /// the load's crosswalk. Empty for a Sleeper league; the poll tick reads
    /// it so it never has to build the crosswalk again.
    pub yahoo_ids: HashMap<String, String>,
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
    /// Cache writes that failed, waiting to be shown to the user.
    ///
    /// A full disk or a read-only data directory used to be completely
    /// silent: every fetch worked, nothing was ever written, and the app just
    /// went back to the wire for 15 MB of players on every launch. The
    /// failures are collected here and drained into the loaded league's
    /// warnings, next to the mock-scoring one.
    pub(crate) cache_warnings: std::sync::Mutex<Vec<(String, String)>>,
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
        Self::with_client(data_dir, SleeperClient::new())
    }

    /// An engine over a client the caller built. The offline tests use it to
    /// point one engine at a dead port without setting proxy variables the
    /// whole process — every other test thread included — would then share.
    pub fn with_client(data_dir: PathBuf, client: SleeperClient) -> Self {
        std::fs::create_dir_all(&data_dir).ok();
        // Everything under here — rosters, league member names, Sleeper user
        // ids, the players dictionary — is the user's alone to read.
        crate::cache::owner_only_dir(&data_dir);
        // A write killed between its temp file and its rename leaves the temp
        // file behind. Nothing ever collected them, so they accumulated in
        // the data directory for the life of the install.
        crate::cache::sweep_stale_temp_files(&data_dir);
        Self {
            client,
            data_dir,
            cache_warnings: std::sync::Mutex::new(Vec::new()),
            key_cache: tokio::sync::Mutex::new(None),
        }
    }

    /// Remember that a cache write failed, at most once per cache file.
    ///
    /// Deduplicated by name rather than by message: the detail carries the
    /// temp file's own unique name, so a poll tick failing to write the same
    /// key every three seconds would otherwise stack up one warning per tick.
    pub(crate) fn note_cache_failure(&self, name: &str, detail: &str) {
        if let Ok(mut warnings) = self.cache_warnings.lock() {
            if warnings.iter().all(|(seen, _)| seen != name) {
                warnings.push((name.to_string(), format!("{name} was not cached: {detail}")));
            }
        }
    }

    /// Take the cache-write failures collected since the last load, so one
    /// load reports each of them once.
    pub(crate) fn take_cache_warnings(&self) -> Vec<String> {
        self.cache_warnings
            .lock()
            .map(|mut w| {
                std::mem::take(&mut *w)
                    .into_iter()
                    .map(|(_, m)| m)
                    .collect()
            })
            .unwrap_or_default()
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
        let final_path = self.cache_path(name);
        let tmp = temp_sibling(&final_path);
        let written =
            tokio::task::spawn_blocking(move || replace_file(tmp, final_path, json)).await;
        if let Ok(Err(error)) = written {
            self.note_cache_failure(name, &error);
        }
        fetched_at
    }

    pub(crate) fn write_cache<T: Serialize>(&self, name: &str, data: &T) -> u64 {
        match self.write_cache_checked(name, data) {
            Ok(fetched_at) => fetched_at,
            Err(error) => {
                self.note_cache_failure(name, &error);
                now_secs()
            }
        }
    }

    pub(crate) fn write_cache_checked<T: Serialize>(
        &self,
        name: &str,
        data: &T,
    ) -> Result<u64, String> {
        let fetched_at = now_secs();
        let final_path = self.cache_path(name);
        let tmp = temp_sibling(&final_path);
        write_atomic(tmp, final_path, fetched_at, data).map_err(|e| format!("{name}: {e}"))?;
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
        let tmp = temp_sibling(&live);
        crate::cache::write_synced(&tmp, json.as_bytes())
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
        let (league, scoring_warning) = synthesize_league(&draft);
        let mut loaded = self
            .assemble(league, draft, HashMap::new(), HashMap::new(), force)
            .await?;
        loaded
            .warnings
            .push("mock draft: league settings synthesized from draft settings".into());
        // A scoring type nobody recognised was scored as standard. That is a
        // full point per catch out on a PPR board, so it goes where the user
        // can see it rather than to stderr.
        loaded.warnings.extend(scoring_warning);
        Ok(loaded)
    }

    /// Load whatever the id turns out to name.
    ///
    /// A Yahoo league key goes down the Yahoo path and needs the connected
    /// client to do it; everything else is a Sleeper league, or failing that a
    /// bare draft id (a mock draft).
    pub async fn load_any(
        &self,
        id: &str,
        force: bool,
        yahoo: Option<&crate::yahoo::YahooClient>,
    ) -> Result<LoadedLeague, String> {
        if crate::view_types::is_yahoo_key(id) {
            let client = yahoo
                .ok_or("that is a Yahoo league — connect your Yahoo account in Settings first")?;
            return self.load_yahoo_league(client, id, force).await;
        }
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
        self.finish_assembly(AssemblyParts {
            league,
            draft,
            user_names,
            user_avatars,
            api_picks,
            traded_picks,
            my_slot: None,
            yahoo_ids: HashMap::new(),
            poll_last_success_at,
            poll_consecutive_failures,
            poll_last_error,
            players: (players_at, player_meta),
            season_projections: (proj_at, season_rows),
            weekly: (weekly_at, weekly_rows),
            warnings,
        })
    }
}

#[cfg(test)]
#[path = "engine_reliability_tests.rs"]
mod reliability_tests;
