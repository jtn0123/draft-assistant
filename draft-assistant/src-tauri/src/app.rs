//! The application behind the desktop commands: one league loaded at a time,
//! the persisted config, and the live poll loop. Nothing here knows about
//! Tauri — `desktop.rs` is a thin shell that forwards each command to a
//! method on [`AppCore`] and turns [`PollEvent`]s into window events — so the
//! whole command surface, including the poll state machine, is testable
//! against a stub Sleeper server.

use crate::chat::{self, ChatOptions, ChatReply, ChatSession, ChatTurn, SessionSummary};
use crate::engine::{self, AppConfig, Engine, LoadedLeague, StoredLeague};
use crate::keepers::note_keepers;
use crate::log;
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
        log::info(format!("add_league {league_id} force={force}"));
        let new_loaded = self
            .engine
            .load_any(&league_id, force)
            .await
            .inspect_err(|error| {
                log::warn(format!("add_league {league_id} failed: {error}"));
            })?;
        log::info(format!(
            "loaded '{}': {} on the board, {} api picks, {} warnings",
            new_loaded.league.name,
            new_loaded.board.len(),
            new_loaded.api_picks.len(),
            new_loaded.warnings.len()
        ));
        log::warnings("load", &new_loaded.warnings);
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
        let started = std::time::Instant::now();
        let (picks, draft) = tokio::join!(
            self.engine.client.picks(&draft_id),
            self.engine.client.draft(&draft_id)
        );
        let picks = picks.inspect_err(|error| {
            log::warn(format!("refresh_picks failed: {error}"));
        })?;

        let mut loaded = self.loaded.lock().await;
        let loaded = loaded.as_mut().ok_or("no league loaded")?;
        loaded.api_picks = picks;
        note_keepers(&self.engine, loaded);
        log::info(format!(
            "refresh_picks in {:.1}s: {} picks, status {}",
            started.elapsed().as_secs_f64(),
            loaded.api_picks.len(),
            loaded.draft.status
        ));
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
        log::info(format!("refresh_data {league_id} (forced refetch)"));
        let started = std::time::Instant::now();
        let new_loaded = self
            .engine
            .load_any(&league_id, true)
            .await
            .inspect_err(|error| {
                log::warn(format!(
                    "refresh_data failed after {:.1}s: {error}",
                    started.elapsed().as_secs_f64()
                ));
            })?;
        log::warnings("refresh_data", &new_loaded.warnings);
        log::info(format!(
            "refresh_data done in {:.1}s: {} on the board, {} api picks, {} warnings",
            started.elapsed().as_secs_f64(),
            new_loaded.board.len(),
            new_loaded.api_picks.len(),
            new_loaded.warnings.len()
        ));
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
        crate::manual::apply_manual_pick(loaded, player_id.clone()).inspect_err(|error| {
            log::warn(format!("manual pick {player_id} refused: {error}"));
        })?;
        if let Some(added) = loaded.manual_picks.last() {
            log::info(format!(
                "manual pick: {player_id} at pick {} (slot {})",
                added.pick_no, added.draft_slot
            ));
        }
        if let Err(error) = self
            .engine
            .save_manual_picks(&loaded.draft.draft_id, &loaded.manual_picks)
        {
            log::warn(format!("manual pick not saved, rolled back: {error}"));
            loaded.manual_picks.pop();
            return Err(error);
        }
        let config = self.config.lock().await;
        Ok(build_view(loaded, &config))
    }

    pub async fn undo_manual_pick(&self) -> Result<DraftView, String> {
        let mut loaded = self.loaded.lock().await;
        let loaded = loaded.as_mut().ok_or("no league loaded")?;
        let removed = crate::manual::undo_manual_pick(loaded).inspect_err(|error| {
            log::warn(format!("undo refused: {error}"));
        })?;
        log::info(format!(
            "undo manual pick {} at {}",
            removed.player_id, removed.pick_no
        ));
        if let Err(error) = self
            .engine
            .save_manual_picks(&loaded.draft.draft_id, &loaded.manual_picks)
        {
            log::warn(format!("undo not saved, restored: {error}"));
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
        let bytes = json.len();
        std::fs::write(&path, json).map_err(|e| {
            log::warn(format!("export_state failed: {e}"));
            e.to_string()
        })?;
        log::info(format!("export_state: {bytes} bytes to {}", path.display()));
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
        log::info(format!(
            "chat ask: {} chars, {} prior turns, model {}, pick {}",
            question.len(),
            history.len(),
            options.model.as_deref().unwrap_or("default"),
            view.draft.current_pick
        ));
        let started = std::time::Instant::now();
        let reply = chat::ask(&view, question, history, options, on_text).await;
        match &reply {
            Ok(reply) => log::info(format!(
                "chat answered in {:.1}s: {} chars, {} context tokens, ${:.2}",
                started.elapsed().as_secs_f64(),
                reply.answer.len(),
                reply.usage.context_tokens,
                reply.usage.cost_usd.unwrap_or(0.0)
            )),
            Err(error) => log::warn(format!(
                "chat failed after {:.1}s: {error}",
                started.elapsed().as_secs_f64()
            )),
        }
        reply
    }

    pub async fn compact(
        &self,
        history: &[ChatTurn],
        options: &ChatOptions,
    ) -> Result<ChatReply, String> {
        log::info(format!("chat compact: {} turns", history.len()));
        let started = std::time::Instant::now();
        let reply = chat::compact(history, options).await;
        match &reply {
            Ok(reply) => log::info(format!(
                "chat compacted in {:.1}s to {} chars, ${:.2}",
                started.elapsed().as_secs_f64(),
                reply.answer.len(),
                reply.usage.cost_usd.unwrap_or(0.0)
            )),
            Err(error) => log::warn(format!("chat compact failed: {error}")),
        }
        reply
    }

    /// Save a conversation; returns the file it went to. Plain file I/O on
    /// the data dir, so no lock is needed and the poll loop is never waited on.
    pub fn save_chat_session(&self, session: &ChatSession) -> Result<String, String> {
        self.engine
            .save_chat_session(session)
            .inspect(|path| {
                log::info(format!(
                    "chat session {} saved: {} turns, ${:.2} -> {path}",
                    session.id,
                    session.turns.len(),
                    session.cost_usd
                ));
            })
            .inspect_err(|error| log::warn(format!("chat session save failed: {error}")))
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
                note_keepers(&self.engine, loaded);
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
            // Only worth a line when it is news: a poll recovering after a
            // failure. Every quiet poll would be three lines a minute.
            if loaded.poll_consecutive_failures > 0 {
                log::info(format!(
                    "poll recovered after {} failures",
                    loaded.poll_consecutive_failures
                ));
            }
            loaded.poll_last_success_at = Some(engine::now_secs());
            loaded.poll_consecutive_failures = 0;
            loaded.poll_last_error = None;
        } else {
            loaded.poll_consecutive_failures = loaded.poll_consecutive_failures.saturating_add(1);
            loaded.poll_last_error = Some(errors.join("; "));
            log::warn(format!(
                "poll failed ({} in a row): {}",
                loaded.poll_consecutive_failures,
                errors.join("; ")
            ));
        }
        let health = view::poll_health(loaded);
        let view = if changed {
            let config = self.config.lock().await;
            let view = build_view(loaded, &config);
            log::info(format!(
                "feed changed: seq {}, status {}, {} picks, on pick {} (slot {}){}",
                view.seq,
                view.draft.status,
                view.draft.total_picks_made,
                view.draft.current_pick,
                view.draft.on_clock_slot,
                if manual_changed {
                    ", manual picks reconciled"
                } else {
                    ""
                }
            ));
            Some(view)
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
        let mut last_season_refresh = std::time::Instant::now();
        log::info(format!(
            "poll loop {generation} started at {interval:?} intervals"
        ));
        while self.poll_active(generation) {
            if let Some((health, view)) = self.poll_once(&mut last_fingerprint).await {
                emit(PollEvent::Health(health));
                if let Some(view) = view {
                    emit(PollEvent::View(Box::new(view)));
                }
            }
            // Once the draft is over the picks feed never changes again; the
            // season side does, on the scale of hours. Slow down, and re-read
            // the calendar, matchups, records and transactions periodically.
            if self.draft_over().await {
                // Projections a day old get the full reload, so the standings
                // and the odds follow the season; otherwise the light one.
                if self.projections_stale().await {
                    last_season_refresh = std::time::Instant::now();
                    match self.refresh_data().await {
                        Ok(view) => emit(PollEvent::View(Box::new(view))),
                        Err(error) => {
                            log::warn(format!("season projections reload failed: {error}"))
                        }
                    }
                } else if last_season_refresh.elapsed() >= crate::app_season::SEASON_REFRESH {
                    last_season_refresh = std::time::Instant::now();
                    match self.refresh_season().await {
                        Ok(view) => emit(PollEvent::View(Box::new(view))),
                        Err(error) => log::warn(format!("season refresh failed: {error}")),
                    }
                }
                tokio::time::sleep(interval.max(crate::app_season::SEASON_IDLE)).await;
            } else {
                tokio::time::sleep(interval).await;
            }
        }
        log::info(format!("poll loop {generation} stopped"));
    }
}
