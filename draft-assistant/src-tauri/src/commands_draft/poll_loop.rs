//! The draft screen's poll loop, and the two commands that start and stop it.
//!
//! Split out of `commands_draft.rs`, which was over the file cap once every
//! command in it gained a logged wrapper. Nothing about the loop changed in
//! the move: it is the same generation bookkeeping, the same backoff, and the
//! same `HealthWatch`.

use super::notes::{NoteWatch, SlowTickWatch};
use super::refusal::{self, refusal_for, Verdict};
use super::seat;
use super::tick::{
    adopt_traded, backoff_secs, build_view_off_lock, draft_update, fetch_tick,
    save_keepers_off_lock, save_picks_off_lock, tick_target, traded_update, DraftUpdate, TickFetch,
    TickTarget,
};
use crate::keepers;
use crate::picks;
use crate::poll::{self, record_poll_outcome, DraftPollMemory};
use crate::state::AppState;
use std::sync::atomic::Ordering;
use std::time::{Duration, Instant};
use tauri::{Emitter, State};

/// Start polling Sleeper picks every `interval_secs` (default 3). Emits a
/// "draft-updated" event with the fresh DraftView whenever anything changed.
#[tauri::command]
pub async fn start_polling<R: tauri::Runtime>(
    app: tauri::AppHandle<R>,
    state: State<'_, AppState>,
    interval_secs: Option<u64>,
) -> Result<(), String> {
    let interval = interval_secs.unwrap_or(3).clamp(2, 60);
    crate::applog::info(format!("polling started every {interval}s"));
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
        // What was last said about this poller's health. Without it the choice
        // is a line every three seconds or, as it was, no line at all.
        let mut watch = crate::applog::HealthWatch::default();
        // The same rule for the notes and for a slow tick: a line when it
        // starts, a line when it stops, nothing in between.
        let mut note_watch = NoteWatch::default();
        let mut slow_watch = SlowTickWatch::default();
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
            if let Some(TickTarget {
                draft_id,
                yahoo_ids,
                users_for,
            }) = target
            {
                let started = Instant::now();
                let fetch_started = std::time::Instant::now();
                let TickFetch {
                    picks,
                    draft,
                    traded,
                    users,
                } = fetch_tick(&engine, &yahoo, &draft_id, &yahoo_ids, users_for.as_deref()).await;
                // The tick boundary at the verbose level: which draft, how
                // many picks came back or what the request said instead, and
                // how long the round trip took.
                crate::applog::debug(format!(
                    "tick fetched draft={draft_id} {} in {}ms",
                    match &picks {
                        Ok(picks) => format!("picks={}", picks.len()),
                        Err(error) => format!("picks=error {error}"),
                    },
                    fetch_started.elapsed().as_millis()
                ));
                let mut changed = false;
                let mut errors = Vec::new();
                // Problems that are worth a log line but are not a failed
                // tick. See `tick::draft_update`.
                let mut notes: Vec<String> = Vec::new();
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
                                // not a cleared board, and an answer with a
                                // hole behind the clock is a partial one, not
                                // a draft that went backwards. Either is
                                // refused, but not forever: see `refusal`.
                                let verdict =
                                    refusal::shared().judge(&draft_id, refusal_for(loaded, &picks));
                                match verdict {
                                    Verdict::Refuse(reason) => errors.push(reason),
                                    Verdict::Adopt(note) => {
                                        // On screen as well as in the log:
                                        // the board is about to move for a
                                        // reason the user never saw.
                                        if let Some(note) = &note {
                                            refusal::warn_adopted(loaded, note);
                                        }
                                        notes.extend(note);
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
                            }
                            Err(error) => errors.push(error),
                        }
                        // The member list the load could not get. Kept out
                        // of `errors` like the two below: the seat labels
                        // are not the picks.
                        match users {
                            Some(Ok(users)) => changed |= seat::adopt_users(loaded, &users),
                            Some(Err(error)) => {
                                notes.push(format!("member list still unavailable: {error}"))
                            }
                            None => {}
                        }
                        // Kept out of `errors` on purpose: only the picks
                        // decide whether this tick failed, so one sulking
                        // `/draft` endpoint cannot grey the sync badge or
                        // stretch the poll to 24 seconds.
                        match draft_update(draft) {
                            DraftUpdate::Adopt(draft) => {
                                changed |= memory.draft_changed(&draft);
                                loaded.draft = *draft;
                            }
                            DraftUpdate::Logged(note) => notes.push(note),
                            DraftUpdate::Refused(reason) => errors.push(reason),
                            DraftUpdate::Nothing => {}
                        }
                        // Picks change hands mid-draft. Kept out of `errors`
                        // for the same reason `/draft` is: a trade list that
                        // does not answer costs nothing, because the one
                        // already on screen is still right.
                        match traded_update(traded) {
                            Ok(Some(traded)) => changed |= adopt_traded(loaded, traded),
                            Ok(None) => {}
                            Err(note) => notes.push(note),
                        }
                        applied = true;
                    }
                }
                // Both files are written with `loaded` let go. Under the lock
                // these were a synchronous disk write on every single tick,
                // three seconds apart, with every command and every view
                // build waiting behind them.
                //
                // A write that fails is a note, never a failed tick. The
                // board in memory is right either way and the next tick
                // writes it again, but counting the failure greyed the sync
                // badge and stretched the poll to 24 seconds over a full
                // disk, as if Sleeper had stopped answering.
                if let Some(picks) = picks_to_save {
                    if let Err(error) = save_picks_off_lock(&engine, draft_id.clone(), picks).await
                    {
                        notes.push(error);
                    }
                }
                if let Some(keepers) = keepers_to_save {
                    notes.extend(save_keepers_off_lock(&engine, draft_id.clone(), keepers).await);
                }
                // Each note once when it appears and once when it clears,
                // rather than every three seconds for as long as it lasts.
                for line in note_watch.observe(&notes) {
                    crate::applog::warn(format!(
                        "{line}{}",
                        crate::applog::context(&[("draft", &draft_id)])
                    ));
                }
                if applied {
                    let mut loaded = loaded_ref.lock().await;
                    if let Some(loaded) = loaded.as_mut().filter(|l| l.draft.draft_id == draft_id) {
                        record_poll_outcome(loaded, &errors);
                        failures = loaded.poll_consecutive_failures;
                        health = Some(poll::poll_health(loaded));
                    }
                }
                if let Some(note) = watch.observe(
                    failures,
                    backoff_secs(interval, failures),
                    errors.first().map(String::as_str),
                ) {
                    crate::applog::warn(format!(
                        "{note}{}",
                        crate::applog::context(&[("draft", &draft_id)])
                    ));
                }
                if let Some(health) = health {
                    app.emit("poll-health", &health).ok();
                    crate::companion::publish(&app, "poll-health", &health);
                }
                if changed {
                    // A copy is taken under the locks and the view is built
                    // off them, on the blocking pool. Built under both
                    // mutexes on a runtime thread, every command and the
                    // other poller waited for the length of the build.
                    let snapshot = {
                        let loaded = loaded_ref.lock().await;
                        let config = config_ref.lock().await;
                        loaded
                            .as_ref()
                            .map(|loaded| (loaded.clone(), config.clone()))
                    };
                    if let Some((loaded, config)) = snapshot {
                        match build_view_off_lock(loaded, config).await {
                            Ok(view) => {
                                app.emit("draft-updated", &view).ok();
                                crate::companion::publish(&app, "draft-updated", &view);
                            }
                            Err(error) => crate::applog::warn(error),
                        }
                    }
                }
                // How long the whole tick took is otherwise invisible: the
                // badge reads success or failure, never how late either was.
                if let Some(line) =
                    slow_watch.observe(started.elapsed(), Duration::from_secs(interval))
                {
                    crate::applog::warn(format!(
                        "{line}{}",
                        crate::applog::context(&[("draft", &draft_id)])
                    ));
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
    crate::applog::info("polling stopped");
    state.polling.store(false, Ordering::SeqCst);
    Ok(())
}
