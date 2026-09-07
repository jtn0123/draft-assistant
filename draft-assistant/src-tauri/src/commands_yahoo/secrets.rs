//! The Keychain side of the Yahoo panel: where the client id, the client
//! secret and the token pair are read and written, and the client built from
//! them.
//!
//! Split out of `commands_yahoo` to keep that file inside the project's line
//! limit. Every read or write here goes through `spawn_blocking` and none is
//! made while the config mutex is held: `security` is a subprocess that can
//! put a password prompt in front of the user, and holding a lock across that
//! stops both pollers for as long as the user takes to answer.

use crate::engine::Engine;
use crate::state::AppState;
use crate::state::YahooState;
use crate::yahoo::YahooClient;
use crate::yahoo_oauth::{TokenSet, YahooCredentials};
use crate::yahoo_secrets::{self, SecretStore};
use std::path::PathBuf;
use std::sync::Arc;

use super::YahooStatus;

/// The Keychain (or its file stand-in) for this install, off the runtime.
///
/// `keychain` is [`YahooState::keychain`]: false pins the file store in the
/// app's own data directory, which is what the tests use and what a machine
/// with no Keychain gets anyway.
pub(super) async fn store_for(
    data_dir: PathBuf,
    keychain: bool,
) -> Result<Arc<dyn SecretStore>, String> {
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
pub(super) async fn read_secrets(
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
pub(super) async fn status_now(state: &AppState) -> Result<YahooStatus, String> {
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
///
/// The read itself is a file open and a JSON parse of a whole team list, and
/// it used to happen on the runtime thread, which is exactly what the rest of
/// this module goes out of its way not to do: it stalls both pollers for as
/// long as the parse takes.
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
    let engine = state.engine.clone();
    tokio::task::spawn_blocking(move || {
        keys.iter().find_map(|key| engine.yahoo_cached_account(key))
    })
    .await
    .ok()
    .flatten()
}

/// The one caller allowed to be building a client at any moment.
///
/// Process-wide rather than a field on [`YahooState`]: there is one of those
/// per app and this is only held for the length of a build, so the only thing
/// it can make wait is a second caller that was about to do the same work.
static BUILD_GATE: std::sync::OnceLock<tokio::sync::Mutex<()>> = std::sync::OnceLock::new();

fn build_gate() -> &'static tokio::sync::Mutex<()> {
    BUILD_GATE.get_or_init(tokio::sync::Mutex::default)
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
    // Check-then-act, so the gate: the load and the first poll tick both find
    // no client, both build one, and only one of them is kept. The loser's
    // client refreshes the access token too, which defeats the "only one
    // refresh in flight" guarantee the client itself makes, and its refreshed
    // pair is then thrown away rather than written to the Keychain.
    let _building = build_gate().lock().await;
    // Look again inside the gate: whoever held it may have built the client
    // while this caller queued, and that one is the one to use.
    if let Some(client) = yahoo.client().await {
        return Ok(client);
    }
    let store = store_for(engine.data_dir.clone(), yahoo.keychain).await?;
    let (credentials, tokens) = read_secrets(store).await?;
    let credentials = credentials.ok_or(
        "Yahoo is not set up: paste your Yahoo app's client id and secret in Settings first",
    )?;
    let tokens = tokens.ok_or("not connected to Yahoo: use Connect in Settings")?;
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
    let Ok(store) = store_for(engine.data_dir.clone(), yahoo.keychain).await else {
        return;
    };
    persist_tokens_into(store, yahoo, client).await;
}

/// [`persist_tokens_for`] against a store the caller already holds.
///
/// Nothing is written when the pair is the one the store already has. The
/// draft poller persists after every tick, and each write is a `security`
/// subprocess against the login Keychain: three seconds apart, all evening,
/// for a pair that changes once an hour.
pub async fn persist_tokens_into(
    store: Arc<dyn SecretStore>,
    yahoo: &YahooState,
    client: &YahooClient,
) {
    // A client Yahoo has signed out is holding a pair that will never work
    // again. Writing it back would keep Settings saying "Connected" and send
    // every later call through the same refusal; clearing it is what makes
    // the next status say "connect again" and mean it.
    if client.signed_out() {
        yahoo.set_client(None).await;
        let cleared =
            tokio::task::spawn_blocking(move || yahoo_secrets::clear_tokens(store.as_ref())).await;
        if let Err(error) = cleared.map_err(|e| e.to_string()).and_then(|r| r) {
            crate::applog::warn(format!("yahoo: dead token not cleared: {error}"));
        }
        return;
    }
    let Some(tokens) = client.unsaved_tokens().await else {
        return;
    };
    let saving = tokens.clone();
    let stored =
        tokio::task::spawn_blocking(move || yahoo_secrets::save_tokens(store.as_ref(), &saving))
            .await;
    match stored.map_err(|e| e.to_string()).and_then(|r| r) {
        // Remembered only once it is really there: a failed write is tried
        // again on the next call rather than believed.
        Ok(()) => client.mark_persisted(&tokens).await,
        Err(error) => crate::applog::warn(format!("yahoo: refreshed token not saved: {error}")),
    }
}
