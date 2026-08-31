//! Tauri commands for the draft screen.

use crate::draft;
use crate::engine::{self, AppConfig, StoredLeague};
use crate::poll::{record_poll_outcome, DraftPollMemory};
use crate::sleeper::Pick;
use crate::state::{view_from, AppState};
use crate::view::{self, DraftView};
use std::sync::atomic::Ordering;
use tauri::{Emitter, State};

/// Pull a Sleeper id out of whatever the user pasted — a bare id, or a URL
/// like `sleeper.com/draft/nfl/1234567890123456789`.
///
/// Anything that is not a run of digits is refused rather than passed through:
/// the result is interpolated straight into a request path, and text that
/// happens to contain `../` would otherwise walk out of `/v1/`.
fn extract_id(input: &str) -> Result<String, String> {
    input
        .split(|c: char| !c.is_ascii_digit())
        .max_by_key(|run| run.len())
        .filter(|run| (15..=25).contains(&run.len()))
        .map(str::to_string)
        .ok_or_else(|| {
            "that doesn't look like a Sleeper ID — paste the league or draft link, \
             or the long number from it"
                .to_string()
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
    let league_id = extract_id(&league_id)?;
    let new_loaded = state.engine.load_any(&league_id, force).await?;
    let mut config = state.config.lock().await;
    if !config.leagues.iter().any(|l| l.league_id == league_id) {
        config.leagues.push(StoredLeague {
            league_id: league_id.clone(),
            name: new_loaded.league.name.clone(),
            season: new_loaded.league.season.clone(),
        });
    }
    config.active_league_id = Some(league_id);
    state.engine.save_config(&config);
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
    let user = state.engine.client.user(&username).await?;
    let mut config = state.config.lock().await;
    config.my_user_id = Some(user.user_id.clone());
    state.engine.save_config(&config);
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
    let draft_id = {
        let loaded = state.loaded.lock().await;
        loaded
            .as_ref()
            .ok_or("no league loaded")?
            .draft
            .draft_id
            .clone()
    };
    let (picks, draft) = tokio::join!(
        state.engine.client.picks(&draft_id),
        state.engine.client.draft(&draft_id)
    );
    let picks = picks?;

    let mut loaded = state.loaded.lock().await;
    let loaded = loaded.as_mut().ok_or("no league loaded")?;
    loaded.api_picks = picks;
    if engine::reconcile_manual_picks(&loaded.api_picks, &mut loaded.manual_picks) {
        state
            .engine
            .save_manual_picks(&draft_id, &loaded.manual_picks)?;
    }
    loaded.poll_last_success_at = Some(engine::now_secs());
    loaded.poll_consecutive_failures = 0;
    loaded.poll_last_error = None;
    // Also refresh draft status/order — it flips to "drafting" at start time.
    if let Ok(draft) = draft {
        loaded.draft = draft;
    }
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
    let new_loaded = state.engine.load_any(&league_id, true).await?;
    let config = state.config.lock().await.clone();
    let view = view_from(&new_loaded, &config);
    *state.loaded.lock().await = Some(new_loaded);
    Ok(view)
}

/// Manual pick fallback for API lag or an offline draft. Marks the given
/// player as taken at the current pick.
#[tauri::command]
pub async fn record_manual_pick(
    state: State<'_, AppState>,
    player_id: String,
) -> Result<DraftView, String> {
    let mut loaded = state.loaded.lock().await;
    let loaded = loaded.as_mut().ok_or("no league loaded")?;
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
    let pick_no = picks.len() as u32 + 1;
    if pick_no > teams * loaded.draft.settings.rounds {
        return Err("draft is complete".into());
    }
    loaded.manual_picks.push(Pick {
        round: (pick_no - 1) / teams + 1,
        pick_no,
        draft_slot: draft::slot_for_pick(pick_no, teams).unwrap_or(1),
        player_id,
        picked_by: None,
        metadata: None,
    });
    if let Err(error) = state
        .engine
        .save_manual_picks(&loaded.draft.draft_id, &loaded.manual_picks)
    {
        loaded.manual_picks.pop();
        return Err(error);
    }
    let config = state.config.lock().await;
    Ok(view_from(loaded, &config))
}

#[tauri::command]
pub async fn undo_manual_pick(state: State<'_, AppState>) -> Result<DraftView, String> {
    let mut loaded = state.loaded.lock().await;
    let loaded = loaded.as_mut().ok_or("no league loaded")?;
    let removed = loaded
        .manual_picks
        .pop()
        .ok_or("no manual picks to undo (API picks cannot be undone locally)")?;
    if let Err(error) = state
        .engine
        .save_manual_picks(&loaded.draft.draft_id, &loaded.manual_picks)
    {
        loaded.manual_picks.push(removed);
        return Err(error);
    }
    let config = state.config.lock().await;
    Ok(view_from(loaded, &config))
}

/// Export the full AI-readable state to a JSON file; returns the path.
#[tauri::command]
pub async fn export_state(state: State<'_, AppState>) -> Result<String, String> {
    let loaded = state.loaded.lock().await;
    let loaded = loaded.as_ref().ok_or("no league loaded")?;
    let config = state.config.lock().await;
    let view = view_from(loaded, &config);
    let path = state.engine.data_dir.join("draft-state.json");
    let json = serde_json::to_string_pretty(&view).map_err(|e| e.to_string())?;
    std::fs::write(&path, json).map_err(|e| e.to_string())?;
    Ok(path.to_string_lossy().to_string())
}

/// Start polling Sleeper picks every `interval_secs` (default 3). Emits a
/// "draft-updated" event with the fresh DraftView whenever anything changed.
#[tauri::command]
pub async fn start_polling(
    app: tauri::AppHandle,
    state: State<'_, AppState>,
    interval_secs: Option<u64>,
) -> Result<(), String> {
    let interval = interval_secs.unwrap_or(3).clamp(2, 60);
    let generation = state.poll_generation.fetch_add(1, Ordering::SeqCst) + 1;
    state.polling.store(true, Ordering::SeqCst);

    let engine = state.engine.clone();
    let loaded_ref = state.loaded.clone();
    let config_ref = state.config.clone();
    let polling = state.polling.clone();
    let poll_generation = state.poll_generation.clone();

    tauri::async_runtime::spawn(async move {
        let mut memory = DraftPollMemory::default();
        loop {
            if !polling.load(Ordering::SeqCst)
                || poll_generation.load(Ordering::SeqCst) != generation
            {
                break;
            }
            let draft_id = {
                let loaded = loaded_ref.lock().await;
                loaded.as_ref().map(|l| l.draft.draft_id.clone())
            };
            if let Some(draft_id) = draft_id {
                let (picks, draft) = tokio::join!(
                    engine.client.picks(&draft_id),
                    engine.client.draft(&draft_id)
                );
                let mut changed = false;
                let mut errors = Vec::new();
                let mut health = None;
                {
                    let mut loaded = loaded_ref.lock().await;
                    if let Some(loaded) = loaded.as_mut() {
                        match picks {
                            Ok(picks) => {
                                changed |= memory.picks_changed(&picks);
                                loaded.api_picks = picks;
                                if engine::reconcile_manual_picks(
                                    &loaded.api_picks,
                                    &mut loaded.manual_picks,
                                ) {
                                    if let Err(error) =
                                        engine.save_manual_picks(&draft_id, &loaded.manual_picks)
                                    {
                                        errors.push(error);
                                    }
                                }
                            }
                            Err(error) => errors.push(error),
                        }
                        match draft {
                            Ok(draft) => {
                                changed |= memory.status_changed(&draft.status);
                                loaded.draft = draft;
                            }
                            Err(error) => errors.push(error),
                        }
                        record_poll_outcome(loaded, &errors);
                        health = Some(view::poll_health(loaded));
                    }
                }
                if let Some(health) = health {
                    app.emit("poll-health", &health).ok();
                }
                if changed {
                    let loaded = loaded_ref.lock().await;
                    let config = config_ref.lock().await;
                    if let Some(loaded) = loaded.as_ref() {
                        let view = view_from(loaded, &config);
                        app.emit("draft-updated", &view).ok();
                    }
                }
            }
            tokio::time::sleep(std::time::Duration::from_secs(interval)).await;
        }
    });
    Ok(())
}

#[tauri::command]
pub async fn stop_polling(state: State<'_, AppState>) -> Result<(), String> {
    state.polling.store(false, Ordering::SeqCst);
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::extract_id;

    #[test]
    fn a_bare_id_and_a_pasted_link_both_work() {
        assert_eq!(
            extract_id("1389710366300200960").unwrap(),
            "1389710366300200960"
        );
        assert_eq!(
            extract_id("https://sleeper.com/draft/nfl/1389710366300200960").unwrap(),
            "1389710366300200960"
        );
        assert_eq!(
            extract_id("  1389710366300200960  ").unwrap(),
            "1389710366300200960"
        );
    }

    #[test]
    fn anything_without_an_id_in_it_is_refused_rather_than_sent_on() {
        // These used to be passed through verbatim and interpolated straight
        // into a request path.
        for junk in [
            "",
            "   ",
            "hello",
            "../../projections/nfl/2025",
            "12345",
            "https://sleeper.com/leagues",
        ] {
            let result = extract_id(junk);
            assert!(
                result.is_err(),
                "{junk:?} should be refused, got {result:?}"
            );
        }
    }

    #[test]
    fn the_error_tells_the_user_what_to_paste() {
        let error = extract_id("nonsense").unwrap_err();
        assert!(error.contains("Sleeper ID"), "unhelpful: {error}");
    }
}
