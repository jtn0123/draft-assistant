//! Tauri commands for the draft screen.

use crate::commands_yahoo::{client_from, persist_tokens_for, yahoo_picks};
use crate::engine::{AppConfig, Engine, LoadedLeague, StoredLeague};
use crate::keepers::{self, KeeperStore};
use crate::league_ref::{extract_ref, Pasted};
use crate::picks::{self, ManualPickStore};
use crate::poll::record_poll_outcome;
use crate::sleeper::{Draft, LeagueUser, Pick};
use crate::sleeper_error::to_message;
use crate::state::{view_from, AppState, YahooState};
use crate::view::DraftView;
use crate::view_types::{is_yahoo_key, platform_for};
use std::collections::{HashMap, HashSet};
use std::sync::Arc;
use tauri::State;

mod edits;
mod notes;
mod poll_loop;
mod rebuild;
mod refusal;
mod seat;
pub(crate) mod tick;
pub use edits::*;
// The generated `__cmd__*` macros come with the commands: `generate_handler!`
// expands a `path::name` to `path::__cmd__name`, so a command re-exported
// without its macro is invisible to every caller that names it by path.
pub use poll_loop::{
    __cmd__start_polling, __cmd__stop_polling, __tauri_command_name_start_polling,
    __tauri_command_name_stop_polling, start_polling, stop_polling,
};
use refusal::{refusal_for, Verdict};
use tick::{
    adopt_traded, build_view_off_lock, draft_update, fetch_tick, save_keepers_off_lock,
    save_picks_off_lock, tick_target, traded_update, view_now, DraftUpdate, TickTarget,
};

/// What every command and tick says when the league moved on under it. The
/// same sentence `same_league` uses on the season side, so the screen shows
/// one wording for one situation.
const LEAGUE_CHANGED: &str = "the league changed while this was loading, try again";

/// The ids a failure on this screen should be tied to, read off the league
/// that is open.
///
/// Most of these commands take no id of their own, so without this a logged
/// failure names the command and nothing else — and "refresh_picks failed" a
/// week later does not say which draft it was.
async fn ids(state: &AppState) -> String {
    let loaded = state.loaded.lock().await;
    let (league, draft) = loaded
        .as_ref()
        .map(|l| (l.league.league_id.clone(), l.draft.draft_id.clone()))
        .unwrap_or_default();
    crate::applog::context(&[("league", &league), ("draft", &draft)])
}

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
                "no league {numeric} on your Yahoo account. Check you are signed in as \
                 the manager who plays in it, or paste the league key (449.l.{numeric})"
            )
        })
}

/// Add (or re-sync) a league by ID, make it active, and build its board.
/// Also accepts a bare draft ID (mock drafts) or a pasted sleeper.com URL.
#[tauri::command]
pub async fn add_league<R: tauri::Runtime>(
    app: tauri::AppHandle<R>,
    state: State<'_, AppState>,
    league_id: String,
    force: Option<bool>,
) -> Result<DraftView, String> {
    let context = crate::applog::context(&[("league", &league_id)]);
    let out = crate::applog::logged!(
        "add_league",
        context,
        add_league_inner(&state, league_id, force).await
    );
    // The phones keep the old league's chat and week until told otherwise.
    if out.is_ok() {
        crate::companion::league_switched(&app).await;
    }
    out
}

async fn add_league_inner(
    state: &AppState,
    league_id: String,
    force: Option<bool>,
) -> Result<DraftView, String> {
    let force = force.unwrap_or(false);
    let league_id = match extract_ref(&league_id)? {
        Pasted::Sleeper(id) | Pasted::Yahoo(id) => id,
        Pasted::YahooNumeric(numeric) => resolve_yahoo_league(state, &numeric).await?,
    };
    let new_loaded = load_dispatched(state, &league_id, force).await?;
    // The line that lets a log be read as a session: everything below a league
    // switch is about a different league, and nothing used to say where the
    // switch happened.
    crate::applog::info(format!(
        "league loaded {} on {}{}",
        new_loaded.league.name,
        platform_for(&league_id),
        crate::applog::context(&[
            ("league", &league_id),
            ("draft", &new_loaded.draft.draft_id),
        ])
    ));
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
    // The write happens before the new config is the one in memory: a save
    // that fails must leave no half-added league behind, and the guard is
    // still held, so no reader can see either version mid-swap. The cost is
    // the disk write under the lock, which a league switch pays once.
    state.engine.prepare_config_save(&next)?.write().await?;
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
    crate::applog::logged!(
        "set_my_username",
        ids(&state).await,
        set_my_username_inner(&state, username).await
    )
}

async fn set_my_username_inner(state: &AppState, username: String) -> Result<String, String> {
    // Through the pooled client, so this call gets the same timeouts, retries
    // and user-agent as every other Sleeper request.
    let user = state
        .engine
        .client
        .user(&username)
        .await
        .map_err(to_message)?;
    save_identity(state, user.user_id.clone()).await?;
    Ok(user.user_id)
}

async fn save_identity(state: &AppState, user_id: String) -> Result<(), String> {
    let mut config = state.config.lock().await;
    let mut next = config.clone();
    next.my_user_id = Some(user_id);
    // Identity is a transactional setting, not optimistic UI state. Keep the
    // ordering lock until this small user-triggered save finishes, so failure
    // cannot change the active roster or roll back another settings writer.
    state.engine.prepare_config_save(&next)?.write().await?;
    *config = next;
    Ok(())
}

#[tauri::command]
pub async fn get_config(state: State<'_, AppState>) -> Result<AppConfig, String> {
    crate::applog::logged!(
        "get_config",
        ids(&state).await,
        get_config_inner(&state).await
    )
}

async fn get_config_inner(state: &AppState) -> Result<AppConfig, String> {
    Ok(state.config.lock().await.clone())
}

/// The one call: full current draft state. This is the UI's data source AND
/// the AI-readable dump.
#[tauri::command]
pub async fn get_state(state: State<'_, AppState>) -> Result<DraftView, String> {
    crate::applog::logged!(
        "get_state",
        ids(&state).await,
        get_state_inner(&state).await
    )
}

async fn get_state_inner(state: &AppState) -> Result<DraftView, String> {
    crate::state::draft_view_snapshot(state, None).await
}

/// Re-poll picks once, right now.
#[tauri::command]
pub async fn refresh_picks(state: State<'_, AppState>) -> Result<DraftView, String> {
    crate::applog::logged!(
        "refresh_picks",
        ids(&state).await,
        refresh_picks_inner(&state).await
    )
}

async fn refresh_picks_inner(state: &AppState) -> Result<DraftView, String> {
    let TickTarget {
        draft_id,
        yahoo_ids,
        users_for,
    } = {
        let loaded = state.loaded.lock().await;
        tick_target(loaded.as_ref().ok_or("no league loaded")?)
    };
    let fetched = fetch_tick(
        &state.engine,
        &state.yahoo,
        &draft_id,
        &yahoo_ids,
        users_for.as_deref(),
    )
    .await;
    let picks = fetched.picks?;

    let mut errors = Vec::new();
    // Problems worth a log line that are nobody's failed tick: a disk write
    // that did not land, and an endpoint beside the picks that did not answer.
    let mut notes: Vec<String> = Vec::new();
    // Everything under the lock is memory work. What has to reach the disk
    // is cloned out and written once the lock is let go, exactly as the poll
    // loop does it: a synchronous write here held every command, every view
    // build and the poller itself behind the disk.
    let (picks_to_save, keepers_to_save, refused) = {
        let mut guard = state.loaded.lock().await;
        let loaded = guard.as_mut().ok_or("no league loaded")?;
        // The requests ran with nothing locked. If the user switched leagues
        // in that window this answer belongs to the old draft, and writing it
        // would put its picks, its manual-pick file and its keepers under the
        // new one.
        if loaded.draft.draft_id != draft_id {
            return Err(LEAGUE_CHANGED.to_string());
        }
        let mut picks_to_save = None;
        let mut keepers_to_save = None;
        let mut refused = None;
        // An empty list mid-draft, or one missing a pick the last answer had
        // with later picks still in it, is a lost or partial answer rather
        // than a shorter draft. Refused, up to a point: see `refusal`.
        let verdict = refusal::shared().judge(&draft_id, refusal_for(loaded, &picks));
        match verdict {
            Verdict::Refuse(reason) => {
                errors.push(reason.clone());
                refused = Some(reason);
            }
            Verdict::Adopt(note) => {
                // The manual re-pull adopts on the same rule the tick does,
                // so it owes the user the same explanation on screen.
                if let Some(note) = &note {
                    refusal::warn_adopted(loaded, note);
                }
                notes.extend(note);
                loaded.api_picks = picks;
                if picks::reconcile_manual_picks(&loaded.api_picks, &mut loaded.manual_picks) {
                    picks_to_save = Some(loaded.manual_picks.clone());
                }
                // A keeper is only recognisable while it sits ahead of the
                // clock, so the judgement is made on every refresh.
                keepers_to_save = keepers::merge_keepers(loaded);
            }
        }
        // Also refresh draft status/order: it flips to "drafting" at start
        // time. A `/draft` that does not answer is logged rather than
        // counted: the picks came through, so this refresh did not fail.
        match draft_update(fetched.draft) {
            DraftUpdate::Adopt(draft) => loaded.draft = *draft,
            DraftUpdate::Logged(note) => notes.push(note),
            DraftUpdate::Refused(reason) => errors.push(reason),
            DraftUpdate::Nothing => {}
        }
        // Trades are agreed mid-draft, so the ownership map is re-read every
        // tick rather than only at load.
        match traded_update(fetched.traded) {
            Ok(Some(traded)) => {
                adopt_traded(loaded, traded);
            }
            Ok(None) => {}
            Err(note) => notes.push(note),
        }
        // The member list the load could not get, if it was asked for.
        match fetched.users {
            Some(Ok(users)) => {
                seat::adopt_users(loaded, &users);
            }
            Some(Err(error)) => notes.push(format!("member list still unavailable: {error}")),
            None => {}
        }
        record_poll_outcome(loaded, &errors);
        (picks_to_save, keepers_to_save, refused)
    };
    // A save that fails is a note. The board in memory is right either way
    // and the next tick writes it again; counting it as a failed poll greyed
    // the sync badge over a full disk.
    if let Some(picks) = picks_to_save {
        if let Err(error) = save_picks_off_lock(&state.engine, draft_id.clone(), picks).await {
            notes.push(error);
        }
    }
    if let Some(keepers) = keepers_to_save {
        notes.extend(save_keepers_off_lock(&state.engine, draft_id.clone(), keepers).await);
    }
    for note in notes {
        crate::applog::warn(note);
    }
    // The answer was refused and the board on screen is the old one. This
    // used to answer Ok with an unchanged view, so the toast said "picks
    // re-pulled: 84 in" over a pull that pulled nothing.
    if let Some(reason) = refused {
        return Err(reason);
    }
    view_now(state, &draft_id).await
}

/// Full data refresh (players + projections + board rebuild).
#[tauri::command]
pub async fn refresh_data(state: State<'_, AppState>) -> Result<DraftView, String> {
    crate::applog::logged!(
        "refresh_data",
        ids(&state).await,
        refresh_data_inner(&state).await
    )
}

async fn refresh_data_inner(state: &AppState) -> Result<DraftView, String> {
    let league_id = {
        let config = state.config.lock().await;
        config.active_league_id.clone().ok_or("no active league")?
    };
    let mut new_loaded = load_dispatched(state, &league_id, true).await?;
    // The rebuild goes back to the wire for everything, which takes long
    // enough for the user to have picked a different league meanwhile. Both
    // locks are taken here, in the order the rest of the app takes them, so
    // the check and the assignment cannot be separated by a switch.
    let mut loaded = state.loaded.lock().await;
    let config = state.config.lock().await;
    if config.active_league_id.as_deref() != Some(league_id.as_str()) {
        return Err(LEAGUE_CHANGED.to_string());
    }
    // The keeper judgement was made when the league was loaded, from where
    // the clock stood then. The rebuild's assembly made it again from where
    // the clock stands now, which mid-draft is a different answer; the one
    // already on screen carries over. See `rebuild`.
    if let Some(previous) = loaded
        .as_ref()
        .filter(|previous| previous.draft.draft_id == new_loaded.draft.draft_id)
    {
        rebuild::carry_keepers(&previous.keeper_pick_nos, &mut new_loaded.keeper_pick_nos);
    }
    // Built from a copy, with both locks let go first. Under the locks this
    // was the stall the poll loop was rewritten to avoid: every undrafted
    // player is copied into the view, and for the length of that copy every
    // command, both pollers and the companion's sockets waited. The copy is
    // cheap, because the board and the dictionaries behind it are shared
    // `Arc`s. See `tick::build_view_off_lock`.
    let copy = new_loaded.clone();
    let config_copy = config.clone();
    *loaded = Some(new_loaded);
    drop(config);
    drop(loaded);
    build_view_off_lock(copy, config_copy).await
}

/// Export the full AI-readable state to a JSON file; returns the path.
#[tauri::command]
pub async fn export_state(state: State<'_, AppState>) -> Result<String, String> {
    crate::applog::logged!(
        "export_state",
        ids(&state).await,
        export_state_inner(&state).await
    )
}

async fn export_state_inner(state: &AppState) -> Result<String, String> {
    let view = crate::state::draft_view_snapshot(state, None).await?;
    // Serialising a whole draft view and writing it out is megabytes of work.
    // Both locks are let go first, so a poll tick landing mid-export is not
    // held up behind the disk.
    let path = state.engine.data_dir.join("draft-state.json");
    let target = path.clone();
    tokio::task::spawn_blocking(move || {
        let json = serde_json::to_string_pretty(&view).map_err(|e| e.to_string())?;
        // Written to a sibling and renamed over, at 0600, exactly like every
        // cache file. A plain `write` truncated the previous export before it
        // had the new bytes, so a full disk or a crash mid-export left the
        // user with an empty file instead of last night's; and the default
        // 0644 published the whole league — member names, Sleeper user ids,
        // every roster — to every account on the machine.
        crate::cache::replace_file(crate::cache::temp_sibling(&target), target, json)
    })
    .await
    .unwrap_or_else(|e| Err(format!("export failed: {e}")))?;
    Ok(path.to_string_lossy().to_string())
}

#[cfg(test)]
#[path = "commands_draft/command_tests.rs"]
mod command_tests;
