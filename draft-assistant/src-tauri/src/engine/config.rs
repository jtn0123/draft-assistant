//! What this app remembers between launches: the settings file, and the list
//! of leagues the picker shows.
//!
//! Split out of `engine.rs`, which was at the file cap. Nothing here changed
//! in the move. Every field added after the first release carries
//! `#[serde(default)]`, because a config written by an older build has to keep
//! loading rather than resetting the user's settings on upgrade.

use crate::engine::Engine;
use serde::{Deserialize, Serialize};
use std::collections::HashMap;
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicU64, Ordering};
use std::sync::{Arc, OnceLock};

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
    /// The port the phone server last took, so a bookmarked URL keeps working.
    #[serde(default)]
    pub companion_port: Option<u16>,
    /// Whether it was on when the app last closed; see COMPANION-API.md.
    #[serde(default)]
    pub companion_enabled: bool,
    /// How much the log writes: `"debug"` or `"info"`. `None` means nobody has
    /// chosen, and `DRAFT_ASSISTANT_DEBUG` decides as it always did. Stored so
    /// that turning verbose logging on to chase something survives the restart
    /// that is usually the next thing the user tries.
    #[serde(default)]
    pub log_level: Option<String>,
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
    /// existed still loads, with every league in it read as a Sleeper one,
    /// which is what it was.
    #[serde(default = "sleeper")]
    pub platform: String,
}

/// The platform a stored league has when its config predates the field.
fn sleeper() -> String {
    crate::view_types::SLEEPER.to_string()
}

/// What reading one settings file found.
enum ConfigFile {
    /// No file at all: a first run, or a live file already set aside.
    Missing,
    /// A file that is there and cannot be used, with the reason.
    Broken(String),
    Parsed(Box<AppConfig>),
}

fn read_config_file(path: &Path) -> ConfigFile {
    match std::fs::read_to_string(path) {
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => ConfigFile::Missing,
        Err(error) => ConfigFile::Broken(error.to_string()),
        Ok(text) => match serde_json::from_str::<AppConfig>(&text) {
            Ok(config) => ConfigFile::Parsed(Box::new(config)),
            Err(error) => ConfigFile::Broken(error.to_string()),
        },
    }
}

/// Move a settings file that cannot be read out of the way, keeping it for
/// the user to recover by hand, and say where it went.
fn set_aside(live: &Path) -> String {
    let kept = live.with_extension(format!("json.broken-{}", crate::engine::now_secs()));
    match std::fs::rename(live, &kept) {
        Ok(()) => kept.display().to_string(),
        Err(error) => format!("{} (could not be moved: {error})", live.display()),
    }
}

impl Engine {
    /// Read the config, falling back to the last good copy if the live file
    /// cannot be used. A key still sitting in the file from before Keychain
    /// storage existed is moved there on the way in.
    ///
    /// A file that is *there* and cannot be parsed is not a first run. It
    /// used to be treated as one: the leagues, the username and every
    /// setting vanished with nothing in the log, and the next save wrote a
    /// fresh file over the only copy of what had been lost. The broken file
    /// is now kept beside the live one under `.broken-<time>`, the loss is
    /// an ERROR line, and the `.bak` is used when it parses.
    pub fn load_config(&self) -> AppConfig {
        let live = self.cache_path("config.json");
        let backup = self.cache_path("config.json.bak");
        let mut config = match read_config_file(&live) {
            ConfigFile::Parsed(config) => *config,
            ConfigFile::Missing => match read_config_file(&backup) {
                ConfigFile::Parsed(config) => {
                    crate::applog::info(
                        "settings read from config.json.bak; config.json is missing",
                    );
                    *config
                }
                ConfigFile::Missing => AppConfig::default(),
                ConfigFile::Broken(reason) => {
                    crate::applog::error(format!(
                        "config.json is missing and config.json.bak cannot be read, starting with default settings: {reason}"
                    ));
                    AppConfig::default()
                }
            },
            ConfigFile::Broken(reason) => {
                let kept = set_aside(&live);
                crate::applog::error(format!(
                    "config.json cannot be read and was set aside as {kept}: {reason}"
                ));
                match read_config_file(&backup) {
                    ConfigFile::Parsed(config) => {
                        crate::applog::warn("settings restored from config.json.bak");
                        *config
                    }
                    _ => {
                        crate::applog::error(
                            "config.json.bak cannot be read either, starting with default settings",
                        );
                        AppConfig::default()
                    }
                }
            }
        };
        if let Some(key) = config.anthropic_api_key.take() {
            // Only ever into this engine's own store, never a Keychain the
            // engine was not built with: an engine over a scratch directory
            // moves the key into a file in that directory.
            let moved = self
                .secrets
                .as_deref()
                .is_some_and(|store| crate::secrets::store_in(store, &key).is_ok());
            if moved {
                // The key is safely in the store either way; if rewriting the
                // file to drop it fails, the next save tries again.
                let _ = self.save_config(&config);
            } else {
                config.anthropic_api_key = Some(key);
            }
        }
        config
    }

    /// Write the config atomically, on this thread: to a temp file first,
    /// then swapped into place, with the previous copy kept as
    /// `config.json.bak`. A crash mid-write can never leave a half-written
    /// config behind.
    ///
    /// Every failure comes back to the caller: a save that quietly did nothing
    /// loses the user's league list at the next launch with nothing said.
    ///
    /// This is the synchronous form, for the two places that have no runtime
    /// to step off: [`Engine::load_config`], which runs once at startup before
    /// anything else holds the config, and the tests. Every command goes
    /// through [`Engine::prepare_config_save`] and writes on the blocking
    /// pool with the config lock already released.
    pub fn save_config(&self, config: &AppConfig) -> Result<(), String> {
        self.prepare_config_save(config)?.write_now()
    }

    /// Everything a save needs, taken while the caller still holds the config
    /// lock: the encoded bytes and a place in the save order. The caller then
    /// drops the lock and awaits [`PendingConfigSave::write`].
    ///
    /// The save used to happen under the lock: encode, write, fsync, back up,
    /// rename, all while every command that reads the config, and both poll
    /// ticks, waited. Once per chat turn, too, because the spend is written
    /// down after every answer. Now only the encode is under the lock.
    ///
    /// The sequence number is what keeps two saves in flight honest. It is
    /// taken here, under the same lock that ordered the two configs, so a
    /// save prepared later always carries the higher number, and a write
    /// that finds a higher number already on disk steps aside rather than
    /// putting a stale config over a newer one.
    pub fn prepare_config_save(&self, config: &AppConfig) -> Result<PendingConfigSave, String> {
        let json = serde_json::to_string_pretty(config)
            .map_err(|e| format!("could not prepare your settings to be saved: {e}"))?;
        let live = self.cache_path("config.json");
        let order = save_order_for(&live);
        let seq = order.next.fetch_add(1, Ordering::SeqCst);
        Ok(PendingConfigSave {
            json,
            backup: self.cache_path("config.json.bak"),
            live,
            seq,
            order,
            before_write: None,
        })
    }
}

/// The save order of one settings file: the next number to hand out, and the
/// number of the save that most recently reached the disk.
///
/// Kept per file path rather than per `Engine` so that two engines over the
/// same directory, which the tests build, agree on the order, and two over
/// different directories never wait on each other.
struct SaveOrder {
    next: AtomicU64,
    /// Held for the whole of a write, so saves of one file land one at a
    /// time and the comparison against it cannot race the rename.
    written: std::sync::Mutex<u64>,
}

fn save_order_for(live: &Path) -> Arc<SaveOrder> {
    static ORDERS: OnceLock<std::sync::Mutex<HashMap<PathBuf, Arc<SaveOrder>>>> = OnceLock::new();
    let mut orders = ORDERS
        .get_or_init(Default::default)
        .lock()
        .unwrap_or_else(|poisoned| poisoned.into_inner());
    orders
        .entry(live.to_path_buf())
        .or_insert_with(|| {
            Arc::new(SaveOrder {
                next: AtomicU64::new(1),
                written: std::sync::Mutex::new(0),
            })
        })
        .clone()
}

/// A config encoded and queued, waiting to be written. Owns everything the
/// write needs, so the config lock it was prepared under can be dropped
/// before the write starts; by construction it cannot hold a guard.
pub struct PendingConfigSave {
    json: String,
    live: PathBuf,
    backup: PathBuf,
    seq: u64,
    order: Arc<SaveOrder>,
    before_write: Option<Box<dyn FnOnce() + Send>>,
}

impl PendingConfigSave {
    /// Write on the blocking pool. The caller must have released the config
    /// lock; nothing here needs it, and holding it would put the wait back.
    pub async fn write(self) -> Result<(), String> {
        tokio::task::spawn_blocking(move || self.write_now())
            .await
            .unwrap_or_else(|e| Err(format!("could not save your settings: {e}")))
    }

    /// The write itself, on whatever thread this is called from.
    pub fn write_now(self) -> Result<(), String> {
        if let Some(hook) = self.before_write {
            hook();
        }
        let mut written = self
            .order
            .written
            .lock()
            .unwrap_or_else(|poisoned| poisoned.into_inner());
        if self.seq < *written {
            // A save prepared after this one has already landed, and it was
            // prepared from a config that included this change. Writing
            // these bytes now would roll the file back.
            return Ok(());
        }
        let tmp = crate::cache::temp_sibling(&self.live);
        crate::cache::write_synced(&tmp, self.json.as_bytes())
            .map_err(|e| format!("could not save your settings to {}: {e}", tmp.display()))?;
        crate::cache::owner_only(&tmp);
        if self.live.exists() {
            crate::cache::back_up(&self.live, &self.backup);
        }
        std::fs::rename(&tmp, &self.live).map_err(|e| {
            format!(
                "could not save your settings to {}: {e}",
                self.live.display()
            )
        })?;
        *written = self.seq;
        Ok(())
    }

    /// Run `hook` on the writing thread just before the file is touched. A
    /// seam for the tests, which park the write here to show that the config
    /// lock is free while a save is in flight; nothing in the app sets it.
    #[doc(hidden)]
    pub fn before_writing(mut self, hook: impl FnOnce() + Send + 'static) -> Self {
        self.before_write = Some(Box::new(hook));
        self
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::path::PathBuf;

    fn test_dir(label: &str) -> PathBuf {
        std::env::temp_dir().join(format!(
            "draft-assistant-config-{label}-{}-{}",
            std::process::id(),
            crate::engine::now_secs()
        ))
    }

    fn broken_files(dir: &Path) -> Vec<PathBuf> {
        std::fs::read_dir(dir)
            .unwrap()
            .flatten()
            .map(|entry| entry.path())
            .filter(|path| {
                path.file_name()
                    .and_then(|name| name.to_str())
                    .is_some_and(|name| name.starts_with("config.json.broken-"))
            })
            .collect()
    }

    /// A config.json that would not parse was treated as a first run: every
    /// league and setting vanished silently, and the next save overwrote the
    /// only copy of them.
    #[test]
    fn an_unreadable_config_is_kept_aside_and_reported_rather_than_treated_as_a_first_run() {
        let dir = test_dir("broken");
        let engine = Engine::new(dir.clone());
        let mut config = AppConfig {
            my_user_id: Some("user-1".into()),
            ..AppConfig::default()
        };
        engine.save_config(&config).unwrap();
        config.my_user_id = Some("user-2".into());
        engine.save_config(&config).unwrap(); // live = user-2, bak = user-1
        std::fs::write(dir.join("config.json"), "{ not json").unwrap();

        let capture = crate::applog::Capture::start();
        let loaded = engine.load_config();
        assert_eq!(
            loaded.my_user_id.as_deref(),
            Some("user-1"),
            "the backup is used when it parses"
        );
        assert!(
            capture.saw("ERROR config.json cannot be read"),
            "{:?}",
            capture.lines()
        );
        drop(capture);
        let kept = broken_files(&dir);
        assert_eq!(
            kept.len(),
            1,
            "the broken file is kept, not overwritten: {kept:?}"
        );
        assert_eq!(std::fs::read_to_string(&kept[0]).unwrap(), "{ not json");
        assert!(
            !dir.join("config.json").exists(),
            "the broken file is moved, so a save cannot overwrite it"
        );
        std::fs::remove_dir_all(dir).unwrap();
    }

    #[test]
    fn a_missing_config_is_a_first_run_and_nothing_is_reported() {
        let dir = test_dir("missing");
        let engine = Engine::new(dir.clone());
        let capture = crate::applog::Capture::start();
        let loaded = engine.load_config();
        assert!(loaded.my_user_id.is_none());
        assert!(
            !capture.lines().iter().any(|line| line.contains("ERROR")),
            "{:?}",
            capture.lines()
        );
        drop(capture);
        assert!(broken_files(&dir).is_empty());
        std::fs::remove_dir_all(dir).unwrap();
    }
}
