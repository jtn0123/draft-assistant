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

use crate::engine::Engine;
use crate::state::YahooState;
use crate::yahoo_oauth::{catch_redirect_on, loopback_port};
use std::net::TcpListener;
use std::sync::Arc;

/// Bind the port a loopback redirect names and finish the sign-in from
/// whatever the browser brings back. A no-op for `oob`.
///
/// The bind happens here, on the caller's thread, rather than in the task:
/// a port that is already taken has to fail the Connect command itself, or
/// the user is sent to Yahoo and comes back to a listener that never existed.
pub(super) fn listen_if_loopback(
    redirect_uri: &str,
    engine: Arc<Engine>,
    yahoo: Arc<YahooState>,
) -> Result<(), String> {
    let Some(port) = loopback_port(redirect_uri) else {
        return Ok(());
    };
    let listener = TcpListener::bind(("127.0.0.1", port)).map_err(|error| {
        format!("could not listen on port {port} for Yahoo's redirect: {error}")
    })?;
    tauri::async_runtime::spawn(async move {
        let caught = tokio::task::spawn_blocking(move || catch_redirect_on(listener)).await;
        let redirect = match caught {
            Ok(Ok(redirect)) => redirect,
            Ok(Err(error)) => return crate::applog::warn(format!("yahoo: redirect: {error}")),
            Err(error) => return crate::applog::warn(format!("yahoo: redirect: {error}")),
        };
        // The code is a secret and stays out of the line; `finish_with` says
        // why it was refused without repeating it.
        if let Err(error) = super::finish_with(&engine, &yahoo, redirect.code, redirect.state).await
        {
            crate::applog::warn(format!("yahoo: loopback sign-in failed: {error}"));
        }
    });
    Ok(())
}
