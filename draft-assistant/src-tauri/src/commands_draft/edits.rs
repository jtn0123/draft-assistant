//! The three commands that change what the draft screen believes: a pick typed
//! in by hand, that pick taken back, and the keeper judgement thrown away.
//!
//! Split out of `commands_draft` to keep that file inside the project's line
//! limit; all three share the same shape, which is why these are the three
//! that moved together. Every one of them writes to disk with the `loaded`
//! lock let go, because on a busy disk the write is tens of milliseconds
//! during which the poll loop and every view build would otherwise be stopped
//! dead.

use super::tick::{save_picks_off_lock, undo_pick_in_memory, view_now};
use super::LEAGUE_CHANGED;
use crate::draft;
use crate::keepers::{self, KeeperStore};
use crate::picks;
use crate::sleeper::Pick;
use crate::state::AppState;
use crate::traded_picks::PickOwnership;
use crate::view::{self, DraftView};
use std::collections::HashSet;
use tauri::State;

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
    // The slot that *owns* this pick, not the one the snake started it on.
    // The plain snake wrote a traded pick down under its original slot, so a
    // reload — which reads `draft_slot` straight off the file — put the pick
    // on the wrong manager's roster, and `recent_picks` named the wrong
    // manager whenever the ownership map could not be rebuilt. This is the
    // same `owner_slot` the view and the simulator use.
    let slot = PickOwnership::from_draft(&loaded.draft, &loaded.traded_picks, teams, rounds, order)
        .owner_slot(pick_no)
        .unwrap_or(1);
    let entered = Pick {
        round: (pick_no - 1) / teams + 1,
        pick_no,
        draft_slot: slot,
        player_id,
        picked_by: None,
        metadata: None,
        is_keeper: None,
    };
    loaded.manual_picks.push(entered.clone());
    let (draft_id, picks) = (loaded.draft.draft_id.clone(), loaded.manual_picks.clone());
    drop(guard);
    // The file write happens with nothing locked: on a slow or busy disk it
    // is tens of milliseconds during which the poll loop, every command and
    // every view build would otherwise be stopped dead.
    if let Err(error) = save_picks_off_lock(&state.engine, draft_id.clone(), picks).await {
        undo_pick_in_memory(&state, &draft_id, &entered).await;
        return Err(crate::applog::failing(
            "record_manual_pick",
            crate::applog::context(&[("draft", &draft_id)]),
        )(error));
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

/// Forget every keeper this app has decided on for the draft on screen, and
/// judge the board again from the picks as they stand now.
///
/// The keeper judgement is deliberately never revisited, because the evidence
/// for it disappears the moment the draft passes the slot. That is right when
/// it was right and unfixable when it was wrong: a league branded from one
/// bad `/picks` answer stayed branded through every relaunch. This is the
/// user's way out.
#[tauri::command]
pub async fn clear_keepers(state: State<'_, AppState>) -> Result<DraftView, String> {
    let draft_id = {
        let loaded = state.loaded.lock().await;
        loaded
            .as_ref()
            .ok_or("no league loaded")?
            .draft
            .draft_id
            .clone()
    };
    // The delete happens with nothing locked, like every other file the draft
    // screen writes.
    let engine = state.engine.clone();
    let removing = draft_id.clone();
    tokio::task::spawn_blocking(move || engine.clear_keepers(&removing))
        .await
        .unwrap_or_else(|error| Err(format!("clearing keepers failed: {error}")))?;

    let mut guard = state.loaded.lock().await;
    let loaded = guard.as_mut().ok_or("no league loaded")?;
    if loaded.draft.draft_id != draft_id {
        return Err(LEAGUE_CHANGED.to_string());
    }
    // Judged again from where the clock is now, so the picks genuinely ahead
    // of it are found and the ones behind it are left alone.
    loaded.keeper_pick_nos = keepers::KeeperMemory {
        picks: HashSet::new(),
        floor: picks::next_open_pick(
            &loaded.api_picks,
            loaded.draft.settings.teams.max(1),
            loaded.draft.settings.rounds.max(1),
        ),
    };
    drop(guard);
    view_now(&state).await
}
