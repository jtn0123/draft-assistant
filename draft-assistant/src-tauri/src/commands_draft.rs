//! Tauri commands for the draft screen.

use crate::commands_yahoo::{client_from, persist_tokens_for, yahoo_picks};
use crate::draft;
use crate::engine::{AppConfig, Engine, LoadedLeague, StoredLeague};
use crate::keepers::{self, KeeperStore};
use crate::league_ref::{extract_ref, Pasted};
use crate::picks::{self, ManualPickStore};
use crate::poll::{self, record_poll_outcome, DraftPollMemory};
use crate::sleeper::{Draft, Pick};
use crate::sleeper_error::to_message;
use crate::state::{view_from, AppState, YahooState};
use crate::view::{self, DraftView};
use crate::view_types::{is_yahoo_key, platform_for};
use std::collections::{HashMap, HashSet};
use std::sync::atomic::Ordering;
use std::sync::Arc;
use tauri::{Emitter, State};

mod tick;
use tick::{
    backoff_secs, fetch_tick, save_keepers_off_lock, save_picks_off_lock, tick_target,
    undo_pick_in_memory, unusable, view_now, EMPTY_PICKS,
};

/// What every command and tick says when the league moved on under it. The
/// same sentence `same_league` uses on the season side, so the screen shows
/// one wording for one situation.
const LEAGUE_CHANGED: &str = "the league changed while this was loading — try again";

/// Load a league on whichever platform its id belongs to.
///
/// The Yahoo client is built here rather than inside the engine so that the
/// tokens it may have refreshed on the way are written back afterwards — the
/// client renews in place, and a renewal nobody stores is spent again on the
/// next launch.
async fn load_dispatched(
    state: &AppState,
    league_id: &str,
    force: bool,
) -> Result<LoadedLeague, String> {
    if !is_yahoo_key(league_id) {
        return state.engine.load_any(league_id, force, None).await;
    }
    let client = client_from(&state.engine, &state.yahoo).await?;
    let loaded = state
        .engine
        .load_any(league_id, force, Some(client.as_ref()))
        .await;
    persist_tokens_for(&state.engine, &state.yahoo, &client).await;
    loaded
}

/// Turn the league id out of a Yahoo URL into the key every Yahoo call takes.
///
/// A URL carries `12345`; the API wants `449.l.12345`, and only the account's
/// own league list knows which game key that is.
async fn resolve_yahoo_league(state: &AppState, numeric: &str) -> Result<String, String> {
    let client = client_from(&state.engine, &state.yahoo).await?;
    let leagues = state.engine.yahoo_user_leagues(&client).await;
    persist_tokens_for(&state.engine, &state.yahoo, &client).await;
    leagues?
        .into_iter()
        .find(|league| league.league_id == numeric)
        .map(|league| league.league_key)
        .ok_or_else(|| {
            format!(
                "no league {numeric} on your Yahoo account — check you are signed in as \
                 the manager who plays in it, or paste the league key (449.l.{numeric})"
            )
        })
}

/// Add (or re-sync) a league by ID, make it active, and build its board.
/// Also accepts a bare draft ID (mock drafts) or a pasted sleeper.com URL.
#[tauri::command]
pub async fn add_league(
    state: State<'_, AppState>,
    league_id: String,
    force: Option<bool>,
) -> Result<DraftView, String> {
    let force = force.unwrap_or(false);
    let league_id = match extract_ref(&league_id)? {
        Pasted::Sleeper(id) | Pasted::Yahoo(id) => id,
        Pasted::YahooNumeric(numeric) => resolve_yahoo_league(&state, &numeric).await?,
    };
    let new_loaded = load_dispatched(&state, &league_id, force).await?;
    let mut config = state.config.lock().await;
    // Edited on a copy and only committed once it is safely on disk: a failed
    // save used to leave the picker showing a league the next launch would
    // not reopen.
    let mut next = config.clone();
    if !next.leagues.iter().any(|l| l.league_id == league_id) {
        next.leagues.push(StoredLeague {
            league_id: league_id.clone(),
            name: new_loaded.league.name.clone(),
            season: new_loaded.league.season.clone(),
            status: Some(new_loaded.league.status.clone()),
            platform: platform_for(&league_id).to_string(),
        });
    } else if let Some(stored) = next.leagues.iter_mut().find(|l| l.league_id == league_id) {
        // A league loaded again has moved on since: it was drafting, now it
        // is in season. The picker should say so.
        stored.name = new_loaded.league.name.clone();
        stored.status = Some(new_loaded.league.status.clone());
        stored.platform = platform_for(&league_id).to_string();
    }
    next.active_league_id = Some(league_id);
    state.engine.save_config(&next)?;
    *config = next;
    let view = view_from(&new_loaded, &config);
    // Never hold config while waiting for loaded: the live path reads loaded first.
    drop(config);
    *state.loaded.lock().await = Some(new_loaded);
    // Season data belongs to the league that was active a moment ago.
    *state.season.lock().await = None;
    Ok(view)
}

/// Identify the user by Sleeper username so "my team" resolves per league.
#[tauri::command]
pub async fn set_my_username(
    state: State<'_, AppState>,
    username: String,
) -> Result<String, String> {
    // Through the pooled client, so this call gets the same timeouts, retries
    // and user-agent as every other Sleeper request.
    let user = state
        .engine
        .client
        .user(&username)
        .await
        .map_err(to_message)?;
    let mut config = state.config.lock().await;
    config.my_user_id = Some(user.user_id.clone());
    state.engine.save_config(&config)?;
    Ok(user.user_id)
}

#[tauri::command]
pub async fn get_config(state: State<'_, AppState>) -> Result<AppConfig, String> {
    Ok(state.config.lock().await.clone())
}

/// The one call: full current draft state. This is the UI's data source AND
/// the AI-readable dump.
#[tauri::command]
pub async fn get_state(state: State<'_, AppState>) -> Result<DraftView, String> {
    let loaded = state.loaded.lock().await;
    let loaded = loaded.as_ref().ok_or("no league loaded")?;
    let config = state.config.lock().await;
    Ok(view_from(loaded, &config))
}

/// Re-poll picks once, right now.
#[tauri::command]
pub async fn refresh_picks(state: State<'_, AppState>) -> Result<DraftView, String> {
    let (draft_id, yahoo_ids) = {
        let loaded = state.loaded.lock().await;
        tick_target(loaded.as_ref().ok_or("no league loaded")?)
    };
    let (picks, draft) = fetch_tick(&state.engine, &state.yahoo, &draft_id, &yahoo_ids).await;
    let picks = picks?;

    let mut loaded = state.loaded.lock().await;
    let loaded = loaded.as_mut().ok_or("no league loaded")?;
    // Both requests ran with nothing locked. If the user switched leagues in
    // that window this answer belongs to the old draft, and writing it would
    // put its picks, its manual-pick file and its keepers under the new one.
    if loaded.draft.draft_id != draft_id {
        return Err(LEAGUE_CHANGED.to_string());
    }
    let mut errors = Vec::new();
    if picks.is_empty() && !loaded.api_picks.is_empty() {
        errors.push(EMPTY_PICKS.to_string());
    } else {
        loaded.api_picks = picks;
        if picks::reconcile_manual_picks(&loaded.api_picks, &mut loaded.manual_picks) {
            state
                .engine
                .save_manual_picks(&draft_id, &loaded.manual_picks)?;
        }
        // A keeper is only recognisable while it sits ahead of the clock, so
        // the judgement is made and written down on every refresh. A keeper
        // set that fails to save is reported here exactly as the background
        // poller reports it — the same tick, collected and recorded, rather
        // than dropped on the floor because this path happens to be the
        // manual one.
        errors.extend(keepers::note_keepers(state.engine.as_ref(), loaded));
    }
    // Also refresh draft status/order — it flips to "drafting" at start time.
    if let Some(Ok(draft)) = draft {
        match unusable(&draft) {
            Some(error) => errors.push(error),
            None => loaded.draft = draft,
        }
    }
    record_poll_outcome(loaded, &errors);
    let config = state.config.lock().await;
    Ok(view_from(loaded, &config))
}

/// Full data refresh (players + projections + board rebuild).
#[tauri::command]
pub async fn refresh_data(state: State<'_, AppState>) -> Result<DraftView, String> {
    let league_id = {
        let config = state.config.lock().await;
        config.active_league_id.clone().ok_or("no active league")?
    };
    let new_loaded = load_dispatched(&state, &league_id, true).await?;
    // The rebuild goes back to the wire for everything, which takes long
    // enough for the user to have picked a different league meanwhile. Both
    // locks are taken here, in the order the rest of the app takes them, so
    // the check and the assignment cannot be separated by a switch.
    let mut loaded = state.loaded.lock().await;
    let config = state.config.lock().await;
    if config.active_league_id.as_deref() != Some(league_id.as_str()) {
        return Err(LEAGUE_CHANGED.to_string());
    }
    let view = view_from(&new_loaded, &config);
    *loaded = Some(new_loaded);
    Ok(view)
}

/// Manual pick fallback for API lag or an offline draft. Marks the given
/// player as taken at the current pick.
#[tauri::command]
pub async fn record_manual_pick(
    state: State<'_, AppState>,
    player_id: String,
) -> Result<DraftView, String> {
    let mut guard = state.loaded.lock().await;
    let loaded = guard.as_mut().ok_or("no league loaded")?;
    let teams = loaded.draft.settings.teams;
    // A manual pick is a correction typed under time pressure; an id that is
    // not on this league's board would be written to disk and reloaded as a
    // ghost pick, so refuse it here.
    if !loaded.board_index.contains_key(&player_id) {
        return Err(format!("player {player_id} is not on this league's board"));
    }
    let picks = view::merged_picks(&loaded.api_picks, &loaded.manual_picks);
    if picks.iter().any(|p| p.player_id == player_id) {
        return Err("player already drafted".into());
    }
    // The first *gap*, not the pick count: keepers already in the book would
    // otherwise push the manual pick several rounds past the real clock.
    let rounds = loaded.draft.settings.rounds;
    let pick_no = view::next_open_pick(&picks, teams, rounds)
        .ok_or_else(|| "draft is complete".to_string())?;
    let (order, _) = draft::DraftOrder::from_draft(&loaded.draft);
    loaded.manual_picks.push(Pick {
        round: (pick_no - 1) / teams + 1,
        pick_no,
        draft_slot: draft::slot_for_pick(pick_no, teams, order).unwrap_or(1),
        player_id,
        picked_by: None,
        metadata: None,
        is_keeper: None,
    });
    let (draft_id, picks) = (loaded.draft.draft_id.clone(), loaded.manual_picks.clone());
    drop(guard);
    // The file write happens with nothing locked: on a slow or busy disk it
    // is tens of milliseconds during which the poll loop, every command and
    // every view build would otherwise be stopped dead.
    if let Err(error) = save_picks_off_lock(&state.engine, draft_id.clone(), picks).await {
        undo_pick_in_memory(&state, &draft_id).await;
        return Err(error);
    }
    view_now(&state).await
}

#[tauri::command]
pub async fn undo_manual_pick(state: State<'_, AppState>) -> Result<DraftView, String> {
    let mut guard = state.loaded.lock().await;
    let loaded = guard.as_mut().ok_or("no league loaded")?;
    let removed = loaded
        .manual_picks
        .pop()
        .ok_or("no manual picks to undo (API picks cannot be undone locally)")?;
    let (draft_id, picks) = (loaded.draft.draft_id.clone(), loaded.manual_picks.clone());
    drop(guard);
    if let Err(error) = save_picks_off_lock(&state.engine, draft_id.clone(), picks).await {
        let mut guard = state.loaded.lock().await;
        if let Some(loaded) = guard.as_mut().filter(|l| l.draft.draft_id == draft_id) {
            loaded.manual_picks.push(removed);
        }
        return Err(error);
    }
    view_now(&state).await
}

/// Export the full AI-readable state to a JSON file; returns the path.
#[tauri::command]
pub async fn export_state(state: State<'_, AppState>) -> Result<String, String> {
    let view = {
        let loaded = state.loaded.lock().await;
        let loaded = loaded.as_ref().ok_or("no league loaded")?;
        let config = state.config.lock().await;
        view_from(loaded, &config)
    };
    // Serialising a whole draft view and writing it out is megabytes of work.
    // Both locks are let go first, so a poll tick landing mid-export is not
    // held up behind the disk.
    let path = state.engine.data_dir.join("draft-state.json");
    let target = path.clone();
    tokio::task::spawn_blocking(move || {
        let json = serde_json::to_string_pretty(&view).map_err(|e| e.to_string())?;
        std::fs::write(&target, json).map_err(|e| e.to_string())
    })
    .await
    .unwrap_or_else(|e| Err(format!("export failed: {e}")))?;
    Ok(path.to_string_lossy().to_string())
}

/// Start polling Sleeper picks every `interval_secs` (default 3). Emits a
/// "draft-updated" event with the fresh DraftView whenever anything changed.
#[tauri::command]
pub async fn start_polling<R: tauri::Runtime>(
    app: tauri::AppHandle<R>,
    state: State<'_, AppState>,
    interval_secs: Option<u64>,
) -> Result<(), String> {
    let interval = interval_secs.unwrap_or(3).clamp(2, 60);
    let generation = state.poll_generation.fetch_add(1, Ordering::SeqCst) + 1;
    state.polling.store(true, Ordering::SeqCst);

    let engine = state.engine.clone();
    let yahoo = state.yahoo.clone();
    let loaded_ref = state.loaded.clone();
    let config_ref = state.config.clone();
    let polling = state.polling.clone();
    let poll_generation = state.poll_generation.clone();

    tauri::async_runtime::spawn(async move {
        let mut memory = DraftPollMemory::default();
        // How many consecutive failures the tick has seen, read back off the
        // loaded league where the poll outcome is recorded.
        let mut failures = 0u32;
        loop {
            if !polling.load(Ordering::SeqCst)
                || poll_generation.load(Ordering::SeqCst) != generation
            {
                break;
            }
            let target = {
                let loaded = loaded_ref.lock().await;
                loaded.as_ref().map(tick_target)
            };
            if let Some((draft_id, yahoo_ids)) = target {
                let (picks, draft) = fetch_tick(&engine, &yahoo, &draft_id, &yahoo_ids).await;
                let mut changed = false;
                let mut errors = Vec::new();
                let mut health = None;
                let mut picks_to_save = None;
                let mut keepers_to_save = None;
                let mut applied = false;
                {
                    let mut loaded = loaded_ref.lock().await;
                    // The requests ran unlocked, so the league on screen may
                    // no longer be the one they were made for. This answer is
                    // then the old league's: applied here it would write the
                    // wrong picks, save the wrong manual-pick file and add the
                    // wrong keepers to the new league's set, on disk. A tick
                    // that arrives too late did not happen at all — nothing is
                    // applied and nothing is recorded.
                    if let Some(loaded) = loaded.as_mut().filter(|l| l.draft.draft_id == draft_id) {
                        match picks {
                            Ok(picks) => {
                                // An empty list mid-draft is a lost response,
                                // not a cleared board.
                                if picks.is_empty() && !loaded.api_picks.is_empty() {
                                    errors.push(EMPTY_PICKS.to_string());
                                } else {
                                    changed |= memory.picks_changed(&picks);
                                    loaded.api_picks = picks;
                                    if picks::reconcile_manual_picks(
                                        &loaded.api_picks,
                                        &mut loaded.manual_picks,
                                    ) {
                                        picks_to_save = Some(loaded.manual_picks.clone());
                                    }
                                    keepers_to_save = keepers::merge_keepers(loaded);
                                }
                            }
                            Err(error) => errors.push(error),
                        }
                        match draft {
                            Some(Ok(draft)) => match unusable(&draft) {
                                Some(error) => errors.push(error),
                                None => {
                                    changed |= memory.status_changed(&draft.status);
                                    loaded.draft = draft;
                                }
                            },
                            Some(Err(error)) => errors.push(error),
                            // Yahoo has no draft resource to refresh.
                            None => {}
                        }
                        applied = true;
                    }
                }
                // Both files are written with `loaded` let go. Under the lock
                // these were a synchronous disk write on every single tick,
                // three seconds apart, with every command and every view
                // build waiting behind them.
                if let Some(picks) = picks_to_save {
                    if let Err(error) = save_picks_off_lock(&engine, draft_id.clone(), picks).await
                    {
                        errors.push(error);
                    }
                }
                if let Some(keepers) = keepers_to_save {
                    errors.extend(save_keepers_off_lock(&engine, draft_id.clone(), keepers).await);
                }
                if applied {
                    let mut loaded = loaded_ref.lock().await;
                    if let Some(loaded) = loaded.as_mut().filter(|l| l.draft.draft_id == draft_id) {
                        record_poll_outcome(loaded, &errors);
                        failures = loaded.poll_consecutive_failures;
                        health = Some(poll::poll_health(loaded));
                    }
                }
                if let Some(health) = health {
                    app.emit("poll-health", &health).ok();
                    crate::companion::publish(&app, "poll-health", &health);
                }
                if changed {
                    let loaded = loaded_ref.lock().await;
                    let config = config_ref.lock().await;
                    if let Some(loaded) = loaded.as_ref() {
                        let view = view_from(loaded, &config);
                        app.emit("draft-updated", &view).ok();
                        crate::companion::publish(&app, "draft-updated", &view);
                    }
                }
            } else {
                // Nothing loaded to poll: the next league starts at full
                // speed rather than inheriting the last one's backoff.
                failures = 0;
            }
            tokio::time::sleep(std::time::Duration::from_secs(backoff_secs(
                interval, failures,
            )))
            .await;
        }
    });
    Ok(())
}

#[tauri::command]
pub async fn stop_polling(state: State<'_, AppState>) -> Result<(), String> {
    state.polling.store(false, Ordering::SeqCst);
    Ok(())
}
