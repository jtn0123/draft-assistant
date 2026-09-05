//! Tauri commands for the draft screen.

use crate::commands_yahoo::{client_from, persist_tokens_for, yahoo_picks};
use crate::engine::{AppConfig, Engine, LoadedLeague, StoredLeague};
use crate::keepers::{self, KeeperStore};
use crate::league_ref::{extract_ref, Pasted};
use crate::picks::{self, ManualPickStore};
use crate::poll::record_poll_outcome;
use crate::sleeper::{Draft, Pick};
use crate::sleeper_error::to_message;
use crate::state::{view_from, AppState, YahooState};
use crate::view::DraftView;
use crate::view_types::{is_yahoo_key, platform_for};
use std::collections::{HashMap, HashSet};
use std::sync::Arc;
use tauri::State;

mod edits;
mod poll_loop;
pub(crate) mod tick;
pub use edits::*;
// The generated `__cmd__*` macros come with the commands: `generate_handler!`
// expands a `path::name` to `path::__cmd__name`, so a command re-exported
// without its macro is invisible to every caller that names it by path.
pub use poll_loop::{
    __cmd__start_polling, __cmd__stop_polling, __tauri_command_name_start_polling,
    __tauri_command_name_stop_polling, start_polling, stop_polling,
};
use tick::{
    adopt_traded, draft_update, fetch_tick, tick_target, traded_update, DraftUpdate, EMPTY_PICKS,
};

/// What every command and tick says when the league moved on under it. The
/// same sentence `same_league` uses on the season side, so the screen shows
/// one wording for one situation.
const LEAGUE_CHANGED: &str = "the league changed while this was loading — try again";

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
    let context = crate::applog::context(&[("league", &league_id)]);
    crate::applog::logged!(
        "add_league",
        context,
        add_league_inner(&state, league_id, force).await
    )
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
    let mut config = state.config.lock().await;
    config.my_user_id = Some(user.user_id.clone());
    state.engine.save_config(&config)?;
    Ok(user.user_id)
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
    let loaded = state.loaded.lock().await;
    let loaded = loaded.as_ref().ok_or("no league loaded")?;
    let config = state.config.lock().await;
    Ok(view_from(loaded, &config))
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
    let (draft_id, yahoo_ids) = {
        let loaded = state.loaded.lock().await;
        tick_target(loaded.as_ref().ok_or("no league loaded")?)
    };
    let fetched = fetch_tick(&state.engine, &state.yahoo, &draft_id, &yahoo_ids).await;
    let picks = fetched.picks?;

    let mut loaded = state.loaded.lock().await;
    let loaded = loaded.as_mut().ok_or("no league loaded")?;
    // Both requests ran with nothing locked. If the user switched leagues in
    // that window this answer belongs to the old draft, and writing it would
    // put its picks, its manual-pick file and its keepers under the new one.
    if loaded.draft.draft_id != draft_id {
        return Err(LEAGUE_CHANGED.to_string());
    }
    let mut errors = Vec::new();
    // Problems worth a log line that are nobody's failed tick: a disk write
    // that did not land, and an endpoint beside the picks that did not answer.
    let mut notes: Vec<String> = Vec::new();
    let kept_previous = picks.is_empty() && !loaded.api_picks.is_empty();
    if kept_previous {
        errors.push(EMPTY_PICKS.to_string());
    } else {
        loaded.api_picks = picks;
        if picks::reconcile_manual_picks(&loaded.api_picks, &mut loaded.manual_picks) {
            // A save that fails is a note. The board in memory is right
            // either way and the next tick writes it again; counting it as a
            // failed poll greyed the sync badge over a full disk.
            notes.extend(
                state
                    .engine
                    .save_manual_picks(&draft_id, &loaded.manual_picks)
                    .err(),
            );
        }
        // A keeper is only recognisable while it sits ahead of the clock, so
        // the judgement is made and written down on every refresh.
        notes.extend(keepers::note_keepers(state.engine.as_ref(), loaded));
    }
    // Also refresh draft status/order — it flips to "drafting" at start time.
    // A `/draft` that does not answer is logged rather than counted: the picks
    // came through, so this refresh did not fail.
    match draft_update(fetched.draft) {
        DraftUpdate::Adopt(draft) => loaded.draft = *draft,
        DraftUpdate::Logged(note) => notes.push(note),
        DraftUpdate::Refused(reason) => errors.push(reason),
        DraftUpdate::Nothing => {}
    }
    // Trades are agreed mid-draft, so the ownership map is re-read every tick
    // rather than only at load.
    match traded_update(fetched.traded) {
        Ok(Some(traded)) => {
            adopt_traded(loaded, traded);
        }
        Ok(None) => {}
        Err(note) => notes.push(note),
    }
    record_poll_outcome(loaded, &errors);
    for note in notes {
        crate::applog::warn(note);
    }
    // The picks came back empty and the board on screen is the old one. This
    // used to answer Ok with an unchanged view, so the toast said "picks
    // re-pulled — 84 in" over a pull that pulled nothing.
    if kept_previous {
        return Err(EMPTY_PICKS.to_string());
    }
    let config = state.config.lock().await;
    Ok(view_from(loaded, &config))
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
    let new_loaded = load_dispatched(state, &league_id, true).await?;
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
mod tests {
    use super::*;

    /// The failure this prevents: a command returned `Err`, the string became
    /// a toast, the toast was dismissed, and nothing anywhere recorded that
    /// the command had been called at all.
    #[test]
    fn a_draft_command_that_fails_leaves_an_error_line_naming_it() {
        let (state, dir) = AppState::scratch("draft-log");
        // The same wrapper the `get_state` command is, with the Tauri `State`
        // it cannot have in a unit test taken out.
        let (out, lines) = crate::applog::captured(|| async {
            crate::applog::logged!(
                "get_state",
                ids(&state).await,
                get_state_inner(&state).await
            )
        });
        assert_eq!(
            out.unwrap_err(),
            "no league loaded",
            "the sentence the user sees is unchanged"
        );
        assert!(
            lines
                .iter()
                .any(|line| line.contains("ERROR get_state failed: no league loaded")),
            "{lines:?}"
        );
        let _ = std::fs::remove_dir_all(&dir);
    }
}
