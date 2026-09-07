//! The loopback half of Connect: listening for the browser when the app is
//! registered with an `http://localhost:<port>/` redirect.
//!
//! Which flow runs is a fact about the redirect URI the app registered
//! ([`crate::yahoo_oauth::REDIRECT_FLOW`], reported on every status), not a
//! second switch: `oob` listens for nothing and the user pastes a code, a
//! loopback URI binds its port before the browser opens and finishes the
//! sign-in when the browser comes back. The app ships `oob` today, so this
//! path runs only in the tests until TRACKER L5 confirms the flow against a
//! real Yahoo app.
//!
//! The listener is kept, by port, so it can be taken down: a Connect the
//! user abandoned used to hold the port for the full five minutes, and a
//! second Connect inside that time failed with "port in use". Starting a new
//! flow or cancelling the old one stops the listener and waits for it to let
//! go of the port before anything binds it again.

use crate::engine::Engine;
use crate::state::YahooState;
use crate::yahoo_oauth::{catch_redirect_on_within_unless, loopback_port, REDIRECT_WAIT};
use std::collections::HashMap;
use std::net::TcpListener;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Arc, Mutex};
use std::time::Duration;
use tauri::async_runtime::JoinHandle;

/// One listener in flight: its cancel flag, and the task to wait on once the
/// flag is set. Keyed by port rather than held on `YahooState` so two
/// sessions in one process (which is how the tests run) cannot cancel each
/// other's: they never share a port.
struct Listening {
    cancel: Arc<AtomicBool>,
    task: JoinHandle<()>,
}

static ACTIVE: Mutex<Option<HashMap<u16, Listening>>> = Mutex::new(None);

/// How long a cancel waits for the old listener to release its port. It
/// polls its flag every 50ms, so this is generous; past it the bind below
/// says "port in use", which is at least the truth.
const RELEASE_WAIT: Duration = Duration::from_secs(2);

fn take(port: u16) -> Option<Listening> {
    let mut active = ACTIVE
        .lock()
        .unwrap_or_else(|poisoned| poisoned.into_inner());
    active.as_mut()?.remove(&port)
}

fn keep(port: u16, listening: Listening) {
    let mut active = ACTIVE
        .lock()
        .unwrap_or_else(|poisoned| poisoned.into_inner());
    active
        .get_or_insert_with(HashMap::new)
        .insert(port, listening);
}

/// Forget the entry for `port` if it is still the one holding `cancel`: a
/// finished task must not remove the listener that replaced it.
fn forget(port: u16, cancel: &Arc<AtomicBool>) {
    let mut active = ACTIVE
        .lock()
        .unwrap_or_else(|poisoned| poisoned.into_inner());
    if let Some(map) = active.as_mut() {
        if map
            .get(&port)
            .is_some_and(|listening| Arc::ptr_eq(&listening.cancel, cancel))
        {
            map.remove(&port);
        }
    }
}

/// Stop the listener on `port`, if there is one, and wait for it to let go.
/// A no-op for `oob`, which never listened.
pub(super) async fn cancel_for(redirect_uri: &str) {
    let Some(port) = loopback_port(redirect_uri) else {
        return;
    };
    let Some(listening) = take(port) else {
        return;
    };
    listening.cancel.store(true, Ordering::SeqCst);
    if tokio::time::timeout(RELEASE_WAIT, listening.task)
        .await
        .is_err()
    {
        crate::applog::warn(format!(
            "yahoo: the redirect listener on port {port} did not stop in time"
        ));
    }
}

/// Bind the port a loopback redirect names and finish the sign-in from
/// whatever the browser brings back. A no-op for `oob`.
///
/// Any listener already on the port is stopped first: a new sign-in is the
/// user starting over, and the old one's browser tab is not coming back.
///
/// The bind happens here, in the command, rather than in the task: a port
/// that is already taken has to fail the Connect command itself, or the user
/// is sent to Yahoo and comes back to a listener that never existed.
pub(super) async fn listen_if_loopback(
    redirect_uri: &str,
    engine: Arc<Engine>,
    yahoo: Arc<YahooState>,
) -> Result<(), String> {
    let Some(port) = loopback_port(redirect_uri) else {
        return Ok(());
    };
    cancel_for(redirect_uri).await;
    let listener = TcpListener::bind(("127.0.0.1", port)).map_err(|error| {
        format!("could not listen on port {port} for Yahoo's redirect: {error}")
    })?;
    let cancel = Arc::new(AtomicBool::new(false));
    let flag = cancel.clone();
    let task = tauri::async_runtime::spawn(async move {
        let watched = flag.clone();
        let caught = tokio::task::spawn_blocking(move || {
            catch_redirect_on_within_unless(listener, REDIRECT_WAIT, &watched)
        })
        .await;
        forget(port, &flag);
        let redirect = match caught {
            Ok(Ok(redirect)) => redirect,
            Ok(Err(error)) => return crate::applog::info(format!("yahoo: redirect: {error}")),
            Err(error) => return crate::applog::warn(format!("yahoo: redirect: {error}")),
        };
        // The code is a secret and stays out of the line; `finish_with` says
        // why it was refused without repeating it.
        if let Err(error) = super::finish_with(&engine, &yahoo, redirect.code, redirect.state).await
        {
            crate::applog::warn(format!("yahoo: loopback sign-in failed: {error}"));
        }
    });
    keep(port, Listening { cancel, task });
    Ok(())
}
