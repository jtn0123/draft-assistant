//! The Settings screen's Yahoo panel: credentials, connect, disconnect, and
//! the account's league list.
//!
//! Yahoo is not Sleeper: there is no anonymous read, so before a league can be
//! loaded the user registers an app at developer.yahoo.com, pastes the client
//! id and secret here, and signs in. Both halves live in the Keychain (see
//! [`crate::yahoo_secrets`]) and neither is ever written to the config, logged,
//! or sent back over the IPC — [`YahooStatus`] answers only *whether* they are
//! there.
//!
//! Every Keychain read or write goes through `spawn_blocking` and none of them
//! is made while the config mutex is held: `security` is a subprocess that can
//! put a password prompt in front of the user, and holding a lock across that
//! stops both pollers for as long as the user takes to answer.

mod loopback;
mod secrets;

use crate::engine::Engine;
use crate::engine::StoredLeague;
use crate::sleeper::Pick;
use crate::state::AppState;
use crate::state::YahooState;
use crate::yahoo_oauth::{authorize_url_on, AuthError, OauthClient, YahooCredentials};
use crate::yahoo_secrets;
use serde::Serialize;
use tauri::State;

pub use secrets::{
    client_for, client_from, persist_tokens, persist_tokens_for, persist_tokens_into,
};
use secrets::{read_secrets, status_now, store_for};

/// What the Settings panel renders itself from.
#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize)]
pub struct YahooStatus {
    /// A client id and secret are stored.
    pub configured: bool,
    /// A token pair is stored, so calls can be made.
    pub connected: bool,
    /// The redirect the app registered with — `oob` unless a test says else.
    pub redirect: String,
    /// The logged-in manager's Yahoo nickname, when a call that knows it has
    /// already been made and cached. Never fetched to answer this.
    pub account: Option<String>,
}

/// What the "Connect Yahoo" button gets back.
#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize)]
pub struct YahooConnectStart {
    pub authorize_url: String,
    pub state: String,
    pub redirect: String,
}

/// One poll tick's worth of Yahoo picks.
///
/// Two calls, because a pick names a team key and only the team list says
/// which draft slot that is. The player ids are the ones the load's crosswalk
/// settled on, carried on the loaded league — rebuilding the crosswalk every
/// three seconds would mean re-indexing the whole Sleeper dictionary.
///
/// The players themselves come off the load's own caches rather than the
/// wire. A tick used to build its picks against an empty player map, so the
/// name, position and team on every pick made after the load were `None` and
/// so was the keeper flag: the board finished loading correctly and then, on
/// the first tick three seconds later, replaced its picks with a set that had
/// forgotten who they were.
pub async fn yahoo_picks(
    engine: &Engine,
    yahoo: &YahooState,
    league_key: &str,
    ids: &std::collections::HashMap<String, String>,
) -> Result<Vec<Pick>, String> {
    let client = client_from(engine, yahoo).await?;
    let (results, teams, players) = tokio::join!(
        client.draft_results(league_key),
        client.league_teams(league_key),
        engine.yahoo_pick_context(league_key),
    );
    // Even a failed call may have spent a refresh token getting there.
    persist_tokens_for(engine, yahoo, &client).await;
    let results = results.map_err(|error| error.to_string())?;
    let teams = teams.map_err(|error| error.to_string())?;
    let mut picks = crate::yahoo_map::picks(&results, &teams, &players);
    for pick in &mut picks {
        if let Some(id) = ids.get(&pick.player_id) {
            pick.player_id = id.clone();
        }
    }
    Ok(picks)
}

/// Whether Yahoo is set up, connected, and who as.
///
/// None of the Yahoo commands has a league or draft id to be tied to, so the
/// context is empty rather than invented. What must never go in it is the one
/// thing these commands do hold: a client secret, a token or an authorization
/// code.
#[tauri::command]
pub async fn yahoo_status(state: State<'_, AppState>) -> Result<YahooStatus, String> {
    crate::applog::logged!("yahoo_status", String::new(), status_now(&state).await)
}

/// Store the client id and secret from developer.yahoo.com.
#[tauri::command]
pub async fn yahoo_save_credentials(
    state: State<'_, AppState>,
    client_id: String,
    client_secret: String,
) -> Result<YahooStatus, String> {
    crate::applog::logged!(
        "yahoo_save_credentials",
        String::new(),
        yahoo_save_credentials_inner(&state, client_id, client_secret).await
    )
}

async fn yahoo_save_credentials_inner(
    state: &AppState,
    client_id: String,
    client_secret: String,
) -> Result<YahooStatus, String> {
    let credentials = YahooCredentials {
        client_id: client_id.trim().to_string(),
        client_secret: client_secret.trim().to_string(),
    };
    if credentials.client_id.is_empty() || credentials.client_secret.is_empty() {
        return Err("both the client id and the client secret are needed".into());
    }
    let store = store_for(state.engine.data_dir.clone(), state.yahoo.keychain).await?;
    tokio::task::spawn_blocking(move || {
        yahoo_secrets::save_credentials(store.as_ref(), &credentials)
    })
    .await
    .map_err(|e| format!("Yahoo credentials: {e}"))??;
    // Any client already built is holding the old identity.
    state.yahoo.set_client(None).await;
    status_now(state).await
}

/// Start the sign-in: the URL to open, and the `state` that comes back with
/// the code.
#[tauri::command]
pub async fn yahoo_begin_connect(state: State<'_, AppState>) -> Result<YahooConnectStart, String> {
    crate::applog::logged!(
        "yahoo_begin_connect",
        String::new(),
        yahoo_begin_connect_inner(&state).await
    )
}

async fn yahoo_begin_connect_inner(state: &AppState) -> Result<YahooConnectStart, String> {
    // The two Keychain reads below used to log themselves, which is now the
    // wrapper's job: doing both wrote the same failure twice.
    let store = store_for(state.engine.data_dir.clone(), state.yahoo.keychain).await?;
    let (credentials, _) = read_secrets(store).await?;
    let credentials = credentials.ok_or(
        "Yahoo is not set up — paste your Yahoo app's client id and secret in Settings first",
    )?;
    let redirect = state.yahoo.hosts.redirect_uri.clone();
    let nonce = nonce()?;
    let authorize_url = authorize_url_on(
        &state.yahoo.hosts.login_base,
        &credentials.client_id,
        &redirect,
        &nonce,
    );
    state.yahoo.expect_state(&nonce).await;
    // A loopback redirect has to be listened for before the browser is sent
    // off, or the one request it makes on the way back finds nobody home.
    loopback::listen_if_loopback(&redirect, state.engine.clone(), state.yahoo.clone()).await?;
    if state.yahoo.open_browser {
        open_in_browser(&authorize_url);
    }
    Ok(YahooConnectStart {
        authorize_url,
        state: nonce,
        redirect,
    })
}

/// Finish the sign-in with the code Yahoo showed the user.
///
/// The managed `AppState` is bound by type rather than by name, so the
/// `state` the frontend sends is this command's own argument — the `state`
/// parameter Yahoo echoed back on the redirect — and not the app's.
#[tauri::command]
pub async fn yahoo_finish_connect(
    app: State<'_, AppState>,
    code: String,
    state: String,
) -> Result<YahooStatus, String> {
    // The code Yahoo showed the user is a secret, so it stays out of the
    // context; the wrapper logs only that this command failed and why.
    crate::applog::logged!(
        "yahoo_finish_connect",
        String::new(),
        yahoo_finish_connect_inner(&app, code, state).await
    )
}

async fn yahoo_finish_connect_inner(
    app: &AppState,
    code: String,
    state: String,
) -> Result<YahooStatus, String> {
    finish_with(&app.engine, &app.yahoo, code, state).await?;
    status_now(app).await
}

/// The exchange itself, on the parts rather than the state: the loopback
/// listener finishes a sign-in from a background task that owns clones of
/// these two and has no `State` to hand.
pub(crate) async fn finish_with(
    engine: &Engine,
    yahoo: &YahooState,
    code: String,
    state: String,
) -> Result<(), String> {
    let Some(expected) = yahoo.take_state().await else {
        return Err("no Yahoo sign-in is in progress — use Connect first".to_string());
    };
    if expected != state.trim() {
        // A reply from some other sign-in is not a typo to correct, so this one
        // stays consumed and the user starts again.
        return Err("that code belongs to a different sign-in — start Connect again".to_string());
    }
    let store = store_for(engine.data_dir.clone(), yahoo.keychain).await?;
    let (credentials, _) = read_secrets(store.clone()).await?;
    let credentials = credentials.ok_or("Yahoo is not set up — save your app credentials first")?;
    let tokens = match OauthClient::with_base(yahoo.hosts.login_base.clone())
        .exchange_code(&credentials, &code, &yahoo.hosts.redirect_uri)
        .await
    {
        Ok(tokens) => tokens,
        Err(error) => {
            // The sign-in itself is still good — Yahoo's code is still on the
            // user's screen — so the pending state goes back and the dialog can
            // take another try at the code rather than sending them round the
            // browser again.
            yahoo.expect_state(&expected).await;
            // The error itself carries whatever Yahoo said back, code and all,
            // which is exactly why it goes through the redacting log — the
            // wrapper above writes the line now, so this returns plainly.
            return Err(match error {
                AuthError::Transport(_) => {
                    format!("could not reach Yahoo to finish signing in — {error}")
                }
                _ => "Yahoo rejected that code — check it and try again".to_string(),
            });
        }
    };
    tokio::task::spawn_blocking(move || yahoo_secrets::save_tokens(store.as_ref(), &tokens))
        .await
        .map_err(|e| format!("Yahoo tokens: {e}"))??;
    // The next call builds a client around the pair just stored.
    yahoo.set_client(None).await;
    Ok(())
}

/// Sign out of the Yahoo account.
///
/// The token goes; the registered app's client id and secret stay, so
/// reconnecting is one click rather than another trip to
/// developer.yahoo.com. `forget_credentials` is the deliberate second step —
/// the "forget the app too" the settings panel asks about separately — and it
/// is the only thing that clears the pair.
#[tauri::command]
pub async fn yahoo_disconnect(
    state: State<'_, AppState>,
    forget_credentials: Option<bool>,
) -> Result<YahooStatus, String> {
    crate::applog::logged!(
        "yahoo_disconnect",
        String::new(),
        yahoo_disconnect_inner(&state, forget_credentials).await
    )
}

async fn yahoo_disconnect_inner(
    state: &AppState,
    forget_credentials: Option<bool>,
) -> Result<YahooStatus, String> {
    let forget = forget_credentials.unwrap_or(false);
    let store = store_for(state.engine.data_dir.clone(), state.yahoo.keychain).await?;
    tokio::task::spawn_blocking(move || match forget {
        true => yahoo_secrets::clear_all(store.as_ref()),
        false => yahoo_secrets::clear_tokens(store.as_ref()),
    })
    .await
    .map_err(|e| format!("Yahoo credentials: {e}"))??;
    state.yahoo.set_client(None).await;
    cancel_connect(&state.yahoo).await;
    status_now(state).await
}

/// Abandon a sign-in that is part-way through: the user closed the Connect
/// dialog. The pending `state` goes, and so does the loopback listener, so
/// the port is free the moment they try again rather than five minutes
/// later. Nothing stored is touched; a connected account stays connected.
#[tauri::command]
pub async fn yahoo_cancel_connect(state: State<'_, AppState>) -> Result<(), String> {
    // Nothing here can fail, so there is no error line for the wrapper the
    // other commands use to write.
    cancel_connect(&state.yahoo).await;
    Ok(())
}

pub(crate) async fn cancel_connect(yahoo: &YahooState) {
    let _ = yahoo.take_state().await;
    loopback::cancel_for(&yahoo.hosts.redirect_uri).await;
}

/// The NFL leagues on the connected account, for the league picker.
#[tauri::command]
pub async fn yahoo_leagues(state: State<'_, AppState>) -> Result<Vec<StoredLeague>, String> {
    crate::applog::logged!(
        "yahoo_leagues",
        String::new(),
        yahoo_leagues_inner(&state).await
    )
}

async fn yahoo_leagues_inner(state: &AppState) -> Result<Vec<StoredLeague>, String> {
    let client = client_for(state).await?;
    let leagues = state.engine.yahoo_user_leagues(&client).await;
    // Even a failed call may have spent a refresh token on the way.
    persist_tokens(state, &client).await;
    Ok(sorted_stored(leagues?))
}

/// What an auction league's picks cost, and what each team had to spend.
///
/// Separate from the board rather than on it: the cost of a pick would belong
/// on `crate::sleeper::Pick` and the budget on its `DraftSettings`, and both
/// of those are Sleeper's shapes, shared with the Sleeper loader. This asks
/// Yahoo the same two questions the board load asks and answers only the
/// auction half, so nothing about a snake draft changes.
///
/// A snake league answers with no budget and no costs, which is how a caller
/// tells: Yahoo describes a live auction as `draft_type: "live"`, so the draft
/// type is not the thing to branch on.
#[tauri::command]
pub async fn yahoo_auction(
    state: State<'_, AppState>,
    league_key: String,
) -> Result<crate::yahoo_map::Auction, String> {
    // Built before the key is handed on, because the macro only evaluates the
    // context on the error path and the key has moved by then.
    let context = crate::applog::context(&[("league", &league_key)]);
    crate::applog::logged!(
        "yahoo_auction",
        context,
        yahoo_auction_inner(&state, league_key).await
    )
}

async fn yahoo_auction_inner(
    state: &AppState,
    league_key: String,
) -> Result<crate::yahoo_map::Auction, String> {
    let client = client_for(state).await?;
    let (league, results) = tokio::join!(
        client.league(&league_key),
        client.draft_results(&league_key)
    );
    // Even a failed call may have spent a refresh token getting there.
    persist_tokens(state, &client).await;
    let mut auction = crate::yahoo_map::auction(
        &league.map_err(|error| error.to_string())?,
        &results.map_err(|error| error.to_string())?,
    );
    // A player the crosswalk matched sits on the board under his Sleeper id,
    // so a cost filed under the Yahoo one would never find him.
    let ids = match state.loaded.lock().await.as_ref() {
        Some(loaded) => loaded.yahoo_ids.clone(),
        None => std::collections::HashMap::new(),
    };
    auction.costs = auction
        .costs
        .into_iter()
        .map(|(id, cost)| (ids.get(&id).cloned().unwrap_or(id), cost))
        .collect();
    Ok(auction)
}

/// Yahoo hands them back in whatever order it likes; the picker wants one a
/// reader can scan, which is the order `leagues::sleeper_leagues` uses.
fn sorted_stored(leagues: Vec<crate::yahoo_types::YahooLeague>) -> Vec<StoredLeague> {
    let mut stored: Vec<StoredLeague> = leagues
        .into_iter()
        .map(|league| StoredLeague {
            league_id: league.league_key,
            name: league.name,
            season: league.season,
            status: Some(crate::yahoo_map::league_status(&league.draft_status)),
            platform: crate::view_types::YAHOO.to_string(),
        })
        .collect();
    stored.sort_by_key(|league| league.name.to_lowercase());
    stored
}

/// The value that ties a redirect to the sign-in that started it: sixteen
/// bytes from the OS random source, as hex.
///
/// It is echoed back through the browser, so it is not a secret, but it does
/// have to be unguessable: on the loopback flow it is the only thing that
/// stops a page the user happens to have open from posting a code of its own
/// choosing to the listener. The clock and the process id, which it used to
/// be built from, are both things such a page can estimate.
fn nonce() -> Result<String, String> {
    let bytes = crate::companion::rand::bytes(16)
        .map_err(|error| format!("could not start a Yahoo sign-in: {error}"))?;
    Ok(bytes.iter().map(|byte| format!("{byte:02x}")).collect())
}

/// Put the authorize URL in front of the user. Best effort on purpose: the
/// command hands the URL back too, and the panel shows it, so a machine where
/// `open` is missing or refuses is inconvenient rather than stuck.
fn open_in_browser(url: &str) {
    #[cfg(target_os = "macos")]
    {
        let _ = std::process::Command::new("/usr/bin/open").arg(url).spawn();
    }
    #[cfg(not(target_os = "macos"))]
    {
        let _ = url;
    }
}

#[cfg(test)]
#[path = "commands_yahoo_tests.rs"]
mod tests;
