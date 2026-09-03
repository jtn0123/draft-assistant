//! Tauri commands for the draft screen.

use crate::draft;
use crate::engine::{AppConfig, StoredLeague};
use crate::keepers;
use crate::picks::{self, ManualPickStore};
use crate::poll::{self, record_poll_outcome, DraftPollMemory};
use crate::sleeper::{Draft, Pick};
use crate::sleeper_error::to_message;
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

/// What a tick says when Sleeper hands back an empty pick list for a draft
/// that already has picks on it. `/picks` answers `null` now and then, which
/// parses as "no picks"; mid-draft that is a lost response rather than every
/// pick being taken back, so the board is kept and the tick counts as failed.
const EMPTY_PICKS: &str = "the pick list came back empty — keeping the picks already on the board";

/// The message for a refreshed draft that cannot be laid out, or `None` when
/// it can be.
///
/// Sleeper serves zero teams and zero rounds for a draft that is still being
/// set up. Every board calculation divides by them, so such a draft is not
/// adopted over one that already works.
fn unusable(draft: &Draft) -> Option<String> {
    let settings = &draft.settings;
    (settings.teams == 0 || settings.rounds == 0).then(|| {
        format!(
            "the draft came back with {} teams and {} rounds — keeping the ones already on screen",
            settings.teams, settings.rounds
        )
    })
}

/// What every command and tick says when the league moved on under it. The
/// same sentence `same_league` uses on the season side, so the screen shows
/// one wording for one situation.
const LEAGUE_CHANGED: &str = "the league changed while this was loading — try again";

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
        });
    } else if let Some(stored) = next.leagues.iter_mut().find(|l| l.league_id == league_id) {
        // A league loaded again has moved on since: it was drafting, now it
        // is in season. The picker should say so.
        stored.name = new_loaded.league.name.clone();
        stored.status = Some(new_loaded.league.status.clone());
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
    let picks = picks.map_err(to_message)?;

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
    if let Ok(draft) = draft {
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
    let new_loaded = state.engine.load_any(&league_id, true).await?;
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
pub async fn start_polling<R: tauri::Runtime>(
    app: tauri::AppHandle<R>,
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
                                        if let Err(error) = engine
                                            .save_manual_picks(&draft_id, &loaded.manual_picks)
                                        {
                                            errors.push(error);
                                        }
                                    }
                                    errors.extend(keepers::note_keepers(engine.as_ref(), loaded));
                                }
                            }
                            Err(error) => errors.push(error.to_string()),
                        }
                        match draft {
                            Ok(draft) => match unusable(&draft) {
                                Some(error) => errors.push(error),
                                None => {
                                    changed |= memory.status_changed(&draft.status);
                                    loaded.draft = draft;
                                }
                            },
                            Err(error) => errors.push(error.to_string()),
                        }
                        record_poll_outcome(loaded, &errors);
                        health = Some(poll::poll_health(loaded));
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
