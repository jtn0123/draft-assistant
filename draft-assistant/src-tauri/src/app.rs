//! The application behind the desktop commands: one league loaded at a time,
//! the persisted config, and the live poll loop. Nothing here knows about
//! Tauri — `desktop.rs` is a thin shell that forwards each command to a
//! method on [`AppCore`] and turns [`PollEvent`]s into window events — so the
//! whole command surface, including the poll state machine, is testable
//! against a stub Sleeper server.

use crate::chat::{self, ChatOptions, ChatReply, ChatSession, ChatTurn, SessionSummary};
use crate::engine::{self, AppConfig, Engine, LoadedLeague, StoredLeague};
use crate::sleeper::extract_id;
use crate::view::{self, build_view, DraftView, PollHealth};
use std::sync::atomic::{AtomicBool, AtomicU64, Ordering};
use std::sync::Arc;
use std::time::Duration;
use tokio::sync::Mutex;

/// What the poll loop reports. `Health` follows every poll; `View` only when
/// the feed changed, so the UI is not re-rendered every three seconds. The
/// view is boxed: it is hundreds of bytes against the health record's few.
#[derive(Debug, Clone)]
pub enum PollEvent {
    Health(PollHealth),
    View(Box<DraftView>),
}

/// Fold newly seen keepers into the league's memory of them: judged from
/// where each pick sits now, and never forgotten once judged.
fn note_keepers(loaded: &mut LoadedLeague) {
    let teams = loaded.draft.settings.teams.max(1);
    let rounds = loaded.draft.settings.rounds.max(1);
    let seen = view::keeper_pick_nos(&loaded.api_picks, teams, rounds);
    loaded.keeper_pick_nos.extend(seen);
}

pub struct AppCore {
    pub engine: Arc<Engine>,
    pub loaded: Arc<Mutex<Option<LoadedLeague>>>,
    pub config: Arc<Mutex<AppConfig>>,
    polling: AtomicBool,
    poll_generation: AtomicU64,
}

impl AppCore {
    pub fn new(engine: Engine) -> Self {
        let config = engine.load_config();
        Self {
            engine: Arc::new(engine),
            loaded: Arc::new(Mutex::new(None)),
            config: Arc::new(Mutex::new(config)),
            polling: AtomicBool::new(false),
            poll_generation: AtomicU64::new(0),
        }
    }

    /// Add (or re-sync) a league by ID, make it active, and build its board.
    /// Also accepts a bare draft ID (mock drafts) or a pasted sleeper.com URL.
    pub async fn add_league(&self, league_id: &str, force: bool) -> Result<DraftView, String> {
        let league_id = extract_id(league_id);
        let new_loaded = self.engine.load_any(&league_id, force).await?;
        let mut config = self.config.lock().await;
        if !config.leagues.iter().any(|l| l.league_id == league_id) {
            config.leagues.push(StoredLeague {
                league_id: league_id.clone(),
                name: new_loaded.league.name.clone(),
                season: new_loaded.league.season.clone(),
            });
        }
        config.active_league_id = Some(league_id);
        self.engine.save_config(&config)?;
        let view = build_view(&new_loaded, &config);
        // Never hold config while waiting for loaded: the live path reads loaded first.
        drop(config);
        *self.loaded.lock().await = Some(new_loaded);
        Ok(view)
    }

    /// Identify the user by Sleeper username so "my team" resolves per league.
    pub async fn set_my_username(&self, username: &str) -> Result<String, String> {
        let user_id = self
            .engine
            .client
            .user_id(username)
            .await?
            .ok_or_else(|| format!("Sleeper user '{username}' not found"))?;
        let mut config = self.config.lock().await;
        config.my_user_id = Some(user_id.clone());
        self.engine.save_config(&config)?;
        Ok(user_id)
    }

    pub async fn get_config(&self) -> AppConfig {
        self.config.lock().await.clone()
    }

    /// The one call: full current draft state. This is the UI's data source AND
    /// the AI-readable dump.
    pub async fn get_state(&self) -> Result<DraftView, String> {
        let loaded = self.loaded.lock().await;
        let loaded = loaded.as_ref().ok_or("no league loaded")?;
        let config = self.config.lock().await;
        Ok(build_view(loaded, &config))
    }

    async fn draft_id(&self) -> Result<String, String> {
        let loaded = self.loaded.lock().await;
        Ok(loaded
            .as_ref()
            .ok_or("no league loaded")?
            .draft
            .draft_id
            .clone())
    }

    /// Re-poll picks once, right now. Unlike the loop, a failed picks fetch
    /// is an error here: the user asked and deserves the answer.
    pub async fn refresh_picks(&self) -> Result<DraftView, String> {
        let draft_id = self.draft_id().await?;
        let (picks, draft) = tokio::join!(
            self.engine.client.picks(&draft_id),
            self.engine.client.draft(&draft_id)
        );
        let picks = picks?;

        let mut loaded = self.loaded.lock().await;
        let loaded = loaded.as_mut().ok_or("no league loaded")?;
        loaded.api_picks = picks;
        note_keepers(loaded);
        if engine::reconcile_manual_picks(&loaded.api_picks, &mut loaded.manual_picks) {
            self.engine
                .save_manual_picks(&draft_id, &loaded.manual_picks)?;
        }
        loaded.poll_last_success_at = Some(engine::now_secs());
        loaded.poll_consecutive_failures = 0;
        loaded.poll_last_error = None;
        // Also refresh draft status/order — it flips to "drafting" at start time.
        if let Ok(draft) = draft {
            loaded.draft = draft;
        }
        let config = self.config.lock().await;
        Ok(build_view(loaded, &config))
    }

    /// Full data refresh (players + projections + board rebuild).
    pub async fn refresh_data(&self) -> Result<DraftView, String> {
        let league_id = {
            let config = self.config.lock().await;
            config.active_league_id.clone().ok_or("no active league")?
        };
        let new_loaded = self.engine.load_any(&league_id, true).await?;
        let config = self.config.lock().await.clone();
        let view = build_view(&new_loaded, &config);
        *self.loaded.lock().await = Some(new_loaded);
        Ok(view)
    }

    /// Manual pick fallback for API lag or an offline draft. Marks the given
    /// player as taken at the current pick. A pick that cannot be persisted
    /// is rolled back: the board must never show a pick the next launch
    /// will have forgotten.
    pub async fn record_manual_pick(&self, player_id: String) -> Result<DraftView, String> {
        let mut loaded = self.loaded.lock().await;
        let loaded = loaded.as_mut().ok_or("no league loaded")?;
        crate::manual::apply_manual_pick(loaded, player_id)?;
        if let Err(error) = self
            .engine
            .save_manual_picks(&loaded.draft.draft_id, &loaded.manual_picks)
        {
            loaded.manual_picks.pop();
            return Err(error);
        }
        let config = self.config.lock().await;
        Ok(build_view(loaded, &config))
    }

    pub async fn undo_manual_pick(&self) -> Result<DraftView, String> {
        let mut loaded = self.loaded.lock().await;
        let loaded = loaded.as_mut().ok_or("no league loaded")?;
        let removed = crate::manual::undo_manual_pick(loaded)?;
        if let Err(error) = self
            .engine
            .save_manual_picks(&loaded.draft.draft_id, &loaded.manual_picks)
        {
            loaded.manual_picks.push(removed);
            return Err(error);
        }
        let config = self.config.lock().await;
        Ok(build_view(loaded, &config))
    }

    /// Export the full AI-readable state to a JSON file; returns the path.
    pub async fn export_state(&self) -> Result<String, String> {
        let view = self.get_state().await?;
        let path = self.engine.data_dir.join("draft-state.json");
        let json = serde_json::to_string_pretty(&view).map_err(|e| e.to_string())?;
        std::fs::write(&path, json).map_err(|e| e.to_string())?;
        Ok(path.to_string_lossy().to_string())
    }

    /// Ask Claude about the current draft. The view is snapshotted and both
    /// locks released before the CLI call: the poll task takes `loaded` every
    /// few seconds and must not wait on a model.
    pub async fn ask(
        &self,
        question: &str,
        history: &[ChatTurn],
        options: &ChatOptions,
        on_text: &mut (dyn FnMut(&str) + Send),
    ) -> Result<ChatReply, String> {
        let view = self.get_state().await?;
        chat::ask(&view, question, history, options, on_text).await
    }

    pub async fn compact(
        &self,
        history: &[ChatTurn],
        options: &ChatOptions,
    ) -> Result<ChatReply, String> {
        chat::compact(history, options).await
    }

    /// Save a conversation; returns the file it went to. Plain file I/O on
    /// the data dir, so no lock is needed and the poll loop is never waited on.
    pub fn save_chat_session(&self, session: &ChatSession) -> Result<String, String> {
        self.engine.save_chat_session(session)
    }

    pub fn list_chat_sessions(&self, draft_id: &str) -> Result<Vec<SessionSummary>, String> {
        self.engine.list_chat_sessions(draft_id)
    }

    pub fn load_chat_session(&self, draft_id: &str, id: &str) -> Result<ChatSession, String> {
        self.engine.load_chat_session(draft_id, id)
    }

    /// One poll: fetch picks and draft, fold them in, account for health, and
    /// say whether the feed changed since `last_fingerprint`. `None` when no
    /// league is loaded. Errors are accumulated into the health record rather
    /// than returned — a poll that fails is news, not an exception.
    pub async fn poll_once(
        &self,
        last_fingerprint: &mut Option<u64>,
    ) -> Option<(PollHealth, Option<DraftView>)> {
        let draft_id = self.draft_id().await.ok()?;
        let (picks, draft) = tokio::join!(
            self.engine.client.picks(&draft_id),
            self.engine.client.draft(&draft_id)
        );
        let mut loaded = self.loaded.lock().await;
        let loaded = loaded.as_mut()?;
        let mut errors = Vec::new();
        // A poll that retires a manual pick has changed what the board shows
        // even when the feed itself is unchanged, and the fingerprint below
        // only covers the feed. Without this the UI keeps rendering a pick
        // the backend has already dropped, and Undo answers "nothing to undo".
        let mut manual_changed = false;
        match picks {
            Ok(picks) => {
                loaded.api_picks = picks;
                note_keepers(loaded);
                if engine::reconcile_manual_picks(&loaded.api_picks, &mut loaded.manual_picks) {
                    manual_changed = true;
                    if let Err(error) = self
                        .engine
                        .save_manual_picks(&draft_id, &loaded.manual_picks)
                    {
                        errors.push(error);
                    }
                }
            }
            Err(error) => errors.push(error),
        }
        match draft {
            Ok(draft) => loaded.draft = draft,
            Err(error) => errors.push(error),
        }
        let fingerprint = view::poll_fingerprint(&loaded.api_picks, &loaded.draft);
        let changed = *last_fingerprint != Some(fingerprint) || manual_changed;
        *last_fingerprint = Some(fingerprint);
        if errors.is_empty() {
            loaded.poll_last_success_at = Some(engine::now_secs());
            loaded.poll_consecutive_failures = 0;
            loaded.poll_last_error = None;
        } else {
            loaded.poll_consecutive_failures = loaded.poll_consecutive_failures.saturating_add(1);
            loaded.poll_last_error = Some(errors.join("; "));
        }
        let health = view::poll_health(loaded);
        let view = if changed {
            let config = self.config.lock().await;
            Some(build_view(loaded, &config))
        } else {
            None
        };
        Some((health, view))
    }

    /// Mark polling on and return the generation the new loop must carry.
    /// Starting again supersedes any loop still running.
    pub fn begin_polling(&self) -> u64 {
        self.polling.store(true, Ordering::SeqCst);
        self.poll_generation.fetch_add(1, Ordering::SeqCst) + 1
    }

    pub fn stop_polling(&self) {
        self.polling.store(false, Ordering::SeqCst);
    }

    fn poll_active(&self, generation: u64) -> bool {
        self.polling.load(Ordering::SeqCst)
            && self.poll_generation.load(Ordering::SeqCst) == generation
    }

    /// Poll every `interval` until stopped or superseded, handing each
    /// outcome to `emit`. A change is judged on the picks' contents and the
    /// draft's status and clock, not the pick count alone, so an edited pick
    /// or a commissioner undo shows up too.
    pub async fn poll_loop(
        &self,
        interval: Duration,
        generation: u64,
        emit: &(dyn Fn(PollEvent) + Send + Sync),
    ) {
        let mut last_fingerprint: Option<u64> = None;
        while self.poll_active(generation) {
            if let Some((health, view)) = self.poll_once(&mut last_fingerprint).await {
                emit(PollEvent::Health(health));
                if let Some(view) = view {
                    emit(PollEvent::View(Box::new(view)));
                }
            }
            tokio::time::sleep(interval).await;
        }
    }
}
