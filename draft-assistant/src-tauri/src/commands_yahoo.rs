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

use crate::engine::Engine;
use crate::engine::StoredLeague;
use crate::sleeper::Pick;
use crate::state::AppState;
use crate::state::YahooState;
use crate::yahoo::YahooClient;
use crate::yahoo_oauth::{authorize_url_on, OauthClient, TokenSet, YahooCredentials};
use crate::yahoo_secrets::{self, SecretStore};
use serde::Serialize;
use std::path::PathBuf;
use std::sync::Arc;
use tauri::State;

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

/// The Keychain (or its file stand-in) for this install, off the runtime.
///
/// `keychain` is [`YahooState::keychain`]: false pins the file store in the
/// app's own data directory, which is what the tests use and what a machine
/// with no Keychain gets anyway.
async fn store_for(data_dir: PathBuf, keychain: bool) -> Result<Arc<dyn SecretStore>, String> {
    tokio::task::spawn_blocking(move || -> Arc<dyn SecretStore> {
        match keychain {
            true => Arc::from(yahoo_secrets::store_for(data_dir)),
            false => Arc::new(yahoo_secrets::FileStore::in_dir(data_dir)),
        }
    })
    .await
    .map_err(|e| format!("Yahoo credentials: {e}"))
}

/// Read both items in one hop off the runtime.
async fn read_secrets(
    store: Arc<dyn SecretStore>,
) -> Result<(Option<YahooCredentials>, Option<TokenSet>), String> {
    tokio::task::spawn_blocking(move || {
        (
            yahoo_secrets::load_credentials(store.as_ref()),
            yahoo_secrets::load_tokens(store.as_ref()),
        )
    })
    .await
    .map_err(|e| format!("Yahoo credentials: {e}"))
}

/// The status as the Keychain and the on-disk caches currently have it.
async fn status_now(state: &AppState) -> Result<YahooStatus, String> {
    let store = store_for(state.engine.data_dir.clone(), state.yahoo.keychain).await?;
    let (credentials, tokens) = read_secrets(store).await?;
    Ok(YahooStatus {
        configured: credentials.is_some(),
        connected: tokens.is_some(),
        redirect: state.yahoo.hosts.redirect_uri.clone(),
        account: cached_account(state).await,
    })
}

/// The manager nickname off whichever Yahoo league this install has already
/// loaded. A cache read, never a request — the settings screen must not wait
/// on Yahoo to render.
async fn cached_account(state: &AppState) -> Option<String> {
    let keys: Vec<String> = {
        let config = state.config.lock().await;
        config
            .leagues
            .iter()
            .filter(|league| league.platform == crate::view_types::YAHOO)
            .map(|league| league.league_id.clone())
            .collect()
    };
    keys.iter()
        .find_map(|key| state.engine.yahoo_cached_account(key))
}

/// The client to make Yahoo calls with, built from the Keychain on first use.
///
/// Every caller must hand the tokens back afterwards through
/// [`persist_tokens`]: the client renews its own access token, and a renewal
/// that is not written down is spent again on the next launch.
pub async fn client_for(state: &AppState) -> Result<Arc<YahooClient>, String> {
    client_from(&state.engine, &state.yahoo).await
}

/// [`client_for`] for a caller that holds the parts rather than the state —
/// the background poll task, which owns clones of both and no `State`.
pub async fn client_from(engine: &Engine, yahoo: &YahooState) -> Result<Arc<YahooClient>, String> {
    if let Some(client) = yahoo.client().await {
        return Ok(client);
    }
    let store = store_for(engine.data_dir.clone(), yahoo.keychain).await?;
    let (credentials, tokens) = read_secrets(store).await?;
    let credentials = credentials.ok_or(
        "Yahoo is not set up — paste your Yahoo app's client id and secret in Settings first",
    )?;
    let tokens = tokens.ok_or("not connected to Yahoo — use Connect in Settings")?;
    let client = Arc::new(YahooClient::with_hosts(
        credentials,
        tokens,
        yahoo.hosts.clone(),
    ));
    yahoo.set_client(Some(client.clone())).await;
    Ok(client)
}

/// Write back whatever the client's last call refreshed.
///
/// A failure here is not worth failing the user's call over — the answer they
/// asked for has already arrived — but it does mean the next launch signs in
/// again, so it goes to stderr rather than nowhere.
pub async fn persist_tokens(state: &AppState, client: &YahooClient) {
    persist_tokens_for(&state.engine, &state.yahoo, client).await;
}

pub async fn persist_tokens_for(engine: &Engine, yahoo: &YahooState, client: &YahooClient) {
    let tokens = client.tokens().await;
    let Ok(store) = store_for(engine.data_dir.clone(), yahoo.keychain).await else {
        return;
    };
    let stored =
        tokio::task::spawn_blocking(move || yahoo_secrets::save_tokens(store.as_ref(), &tokens))
            .await;
    if let Err(error) = stored.map_err(|e| e.to_string()).and_then(|r| r) {
        eprintln!("yahoo: refreshed token not saved: {error}");
    }
}

/// One poll tick's worth of Yahoo picks.
///
/// Two calls, because a pick names a team key and only the team list says
/// which draft slot that is. The player ids are the ones the load's crosswalk
/// settled on, carried on the loaded league — rebuilding the crosswalk every
/// three seconds would mean re-indexing the whole Sleeper dictionary.
pub async fn yahoo_picks(
    engine: &Engine,
    yahoo: &YahooState,
    league_key: &str,
    ids: &std::collections::HashMap<String, String>,
) -> Result<Vec<Pick>, String> {
    let client = client_from(engine, yahoo).await?;
    let (results, teams) = tokio::join!(
        client.draft_results(league_key),
        client.league_teams(league_key)
    );
    // Even a failed call may have spent a refresh token getting there.
    persist_tokens_for(engine, yahoo, &client).await;
    let results = results.map_err(|error| error.to_string())?;
    let teams = teams.map_err(|error| error.to_string())?;
    let mut picks = crate::yahoo_map::picks(&results, &teams, &std::collections::HashMap::new());
    for pick in &mut picks {
        if let Some(id) = ids.get(&pick.player_id) {
            pick.player_id = id.clone();
        }
    }
    Ok(picks)
}

/// Whether Yahoo is set up, connected, and who as.
#[tauri::command]
pub async fn yahoo_status(state: State<'_, AppState>) -> Result<YahooStatus, String> {
    status_now(&state).await
}

/// Store the client id and secret from developer.yahoo.com.
#[tauri::command]
pub async fn yahoo_save_credentials(
    state: State<'_, AppState>,
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
    status_now(&state).await
}

/// Start the sign-in: the URL to open, and the `state` that comes back with
/// the code.
#[tauri::command]
pub async fn yahoo_begin_connect(state: State<'_, AppState>) -> Result<YahooConnectStart, String> {
    let store = store_for(state.engine.data_dir.clone(), state.yahoo.keychain).await?;
    let (credentials, _) = read_secrets(store).await?;
    let credentials = credentials.ok_or(
        "Yahoo is not set up — paste your Yahoo app's client id and secret in Settings first",
    )?;
    let redirect = state.yahoo.hosts.redirect_uri.clone();
    let nonce = nonce();
    let authorize_url = authorize_url_on(
        &state.yahoo.hosts.login_base,
        &credentials.client_id,
        &redirect,
        &nonce,
    );
    state.yahoo.expect_state(&nonce).await;
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
    let expected = app.yahoo.take_state().await;
    match expected {
        Some(expected) if expected == state.trim() => {}
        Some(_) => {
            return Err(
                "that code belongs to a different sign-in — start Connect again".to_string(),
            )
        }
        None => return Err("no Yahoo sign-in is in progress — use Connect first".to_string()),
    }
    let store = store_for(app.engine.data_dir.clone(), app.yahoo.keychain).await?;
    let (credentials, _) = read_secrets(store.clone()).await?;
    let credentials = credentials.ok_or("Yahoo is not set up — save your app credentials first")?;
    let tokens = OauthClient::with_base(app.yahoo.hosts.login_base.clone())
        .exchange_code(&credentials, &code, &app.yahoo.hosts.redirect_uri)
        .await
        .map_err(|error| error.to_string())?;
    tokio::task::spawn_blocking(move || yahoo_secrets::save_tokens(store.as_ref(), &tokens))
        .await
        .map_err(|e| format!("Yahoo tokens: {e}"))??;
    // The next call builds a client around the pair just stored.
    app.yahoo.set_client(None).await;
    status_now(&app).await
}

/// Forget the tokens and the credentials both.
#[tauri::command]
pub async fn yahoo_disconnect(state: State<'_, AppState>) -> Result<YahooStatus, String> {
    let store = store_for(state.engine.data_dir.clone(), state.yahoo.keychain).await?;
    tokio::task::spawn_blocking(move || yahoo_secrets::clear_all(store.as_ref()))
        .await
        .map_err(|e| format!("Yahoo credentials: {e}"))??;
    state.yahoo.set_client(None).await;
    let _ = state.yahoo.take_state().await;
    status_now(&state).await
}

/// The NFL leagues on the connected account, for the league picker.
#[tauri::command]
pub async fn yahoo_leagues(state: State<'_, AppState>) -> Result<Vec<StoredLeague>, String> {
    let client = client_for(&state).await?;
    let leagues = state.engine.yahoo_user_leagues(&client).await;
    // Even a failed call may have spent a refresh token on the way.
    persist_tokens(&state, &client).await;
    Ok(sorted_stored(leagues?))
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

/// One unguessable-enough value to tie a redirect to the request that started
/// it. Not a secret: it is echoed back through the browser, and its whole job
/// is to be different every time.
fn nonce() -> String {
    use std::sync::atomic::{AtomicU64, Ordering};
    static COUNTER: AtomicU64 = AtomicU64::new(0);
    let now = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map(|d| d.as_nanos())
        .unwrap_or(0);
    let count = COUNTER.fetch_add(1, Ordering::SeqCst);
    format!("{now:x}{count:x}{:x}", std::process::id())
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
