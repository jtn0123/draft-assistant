//! The shared chat over HTTP, and the one path every shared question takes.
//!
//! A question is answered *off* the request: the phone gets its 202 as soon as
//! the question is in the thread, and the answer arrives later on the
//! WebSocket. Anything else would hold a mobile connection open for the length
//! of a model call.

use super::routes::{fail, Auth};
use super::server::Srv;
use crate::chat::ChatReply;
use crate::shared_chat::{EntryDevice, PostError};
use axum::extract::{Query, State};
use axum::http::StatusCode;
use axum::response::{IntoResponse, Response};
use axum::Json;
use serde::Deserialize;
use std::sync::Arc;
use std::time::Duration;

/// How long a shared question may run before the thread stops waiting for it.
///
/// Generous next to a model call and short next to "forever": nothing used to
/// bound this, so an answer that hung left `busy` set, and every paired
/// device's composer greyed out, until the app was restarted.
const ANSWER_TIMEOUT: Duration = Duration::from_secs(300);

/// The screens a shared question may be asked about — the same two the desktop
/// panel answers for.
fn check_screen(screen: &str) -> Result<&'static str, String> {
    match screen {
        "draft" => Ok("draft"),
        "season" => Ok("season"),
        other => Err(format!("'{other}' is not a screen Ask Claude answers for")),
    }
}

/// The league the shared thread belongs to. There is no thread without one:
/// a question about "the board" means nothing until a board is open.
pub async fn active_league(srv: &Srv) -> Result<String, String> {
    let loaded = srv.state.loaded.lock().await;
    Ok(loaded
        .as_ref()
        .ok_or("no league loaded")?
        .league
        .league_id
        .clone())
}

#[derive(Deserialize)]
pub struct ScreenQuery {
    #[serde(default)]
    screen: String,
}

pub async fn get_chat(
    State(srv): State<Arc<Srv>>,
    _auth: Auth,
    Query(query): Query<ScreenQuery>,
) -> Response {
    let screen = match check_screen(&query.screen) {
        Ok(screen) => screen,
        Err(e) => return fail(StatusCode::BAD_REQUEST, &e),
    };
    match active_league(&srv).await {
        Ok(league_id) => Json(srv.chat.thread(&league_id, screen).await).into_response(),
        Err(e) => fail(StatusCode::NOT_FOUND, &e),
    }
}

#[derive(Deserialize)]
pub struct AskBody {
    #[serde(default)]
    screen: String,
    #[serde(default)]
    text: String,
}

pub async fn post_chat(
    State(srv): State<Arc<Srv>>,
    Auth(device): Auth,
    Json(body): Json<AskBody>,
) -> Response {
    let screen = match check_screen(&body.screen) {
        Ok(screen) => screen,
        Err(e) => return fail(StatusCode::BAD_REQUEST, &e),
    };
    // Ten questions a minute, per device: the answers cost the host money, and
    // the cap is per device so one phone cannot spend the whole league's.
    if !srv.hub.allow_chat_post(&device.device_id) {
        return fail(
            StatusCode::TOO_MANY_REQUESTS,
            "too many questions — wait a moment",
        );
    }
    let asked_by = EntryDevice {
        name: device.name.clone(),
        kind: device.kind.clone(),
    };
    match ask(srv.clone(), screen, asked_by, body.text).await {
        Ok(entry_id) => (
            StatusCode::ACCEPTED,
            Json(serde_json::json!({ "entry_id": entry_id })),
        )
            .into_response(),
        Err(AskError::Busy) => fail(StatusCode::CONFLICT, "busy"),
        Err(AskError::NoLeague(e)) => fail(StatusCode::NOT_FOUND, &e),
        Err(AskError::BadText(e)) => fail(StatusCode::BAD_REQUEST, &e),
    }
}

pub enum AskError {
    Busy,
    NoLeague(String),
    BadText(String),
}

impl AskError {
    /// The message a Tauri command shows. The HTTP side keeps the status codes
    /// the contract names; the desktop only ever sees a string.
    pub fn message(self) -> String {
        match self {
            AskError::Busy => {
                "someone else is asking a question — try again in a moment".to_string()
            }
            AskError::NoLeague(e) | AskError::BadText(e) => e,
        }
    }
}

/// Put a question in the shared thread and start answering it.
///
/// The one entry point for both the phone and the host's own panel, so both
/// get the same busy rule, the same broadcast, and the same accounting.
/// Returns as soon as the question is in the thread; the answer follows on the
/// WebSocket and the `shared-chat` webview event.
pub async fn ask(
    srv: Arc<Srv>,
    screen: &'static str,
    device: EntryDevice,
    text: String,
) -> Result<String, AskError> {
    let league_id = active_league(&srv).await.map_err(AskError::NoLeague)?;
    let (entry_id, thread) = srv
        .chat
        .post(&league_id, screen, device.clone(), &text)
        .await
        .map_err(|e| match e {
            PostError::Busy => AskError::Busy,
            PostError::BadText(message) => AskError::BadText(message),
        })?;
    // The question is on every screen before the model has been called, which
    // is what makes the shared thread feel shared.
    srv.announce(&thread);
    tokio::spawn(answer_and_finish(
        srv,
        screen,
        league_id,
        device,
        ANSWER_TIMEOUT,
    ));
    Ok(entry_id)
}

/// Ask the model and file the result in the thread.
pub async fn answer_and_finish(
    srv: Arc<Srv>,
    screen: &'static str,
    league_id: String,
    device: EntryDevice,
    limit: Duration,
) {
    let work = {
        let srv = srv.clone();
        let league_id = league_id.clone();
        async move {
            let messages = srv.chat.messages(&league_id, screen).await;
            // Model and effort are the backend's defaults: the desktop panel
            // keeps its model picker in the frontend, so there is no host-side
            // setting to inherit. Everything that *is* host-side — provider,
            // key, budget cap and the spend it is checked against — comes
            // from `answer`.
            crate::commands_chat::answer(&srv.state, screen, "", "", messages).await
        }
    };
    finish_within(srv, screen, league_id, device, limit, work).await;
}

/// Run `work`, put whatever it produces in the thread, and clear `busy`
/// however it ends — answered, failed, timed out, or panicking.
///
/// Public and generic over the work so the tests can hand it a future that
/// never finishes, which is the case that used to wedge the thread.
pub async fn finish_within<F>(
    srv: Arc<Srv>,
    screen: &'static str,
    league_id: String,
    device: EntryDevice,
    limit: Duration,
    work: F,
) where
    F: std::future::Future<Output = Result<ChatReply, String>> + Send + 'static,
{
    let mut guard = BusyGuard {
        srv: Some(srv.clone()),
        league_id: league_id.clone(),
        screen,
        device: device.clone(),
    };
    // On its own task so a panic in the answer comes back as a JoinError
    // rather than unwinding through the thread's bookkeeping.
    let running = tokio::spawn(work);
    let abort = running.abort_handle();
    let reply = match tokio::time::timeout(limit, running).await {
        Ok(Ok(reply)) => reply,
        Ok(Err(_)) => Err("The answer stopped unexpectedly".to_string()),
        Err(_) => {
            // Nothing is going to read what it eventually produces, and it is
            // still holding this league's one in-flight claim.
            abort.abort();
            Err("Timed out waiting for an answer".to_string())
        }
    };
    guard.disarm();
    let thread = srv.chat.finish(&league_id, screen, device, reply).await;
    srv.announce(&thread);
}

/// Files a failure if the answering task ends without one being filed.
///
/// `finish` is async and `Drop` is not, so the guard hands the clearing work
/// to a task of its own. Disarmed on the ordinary path, where the caller files
/// the real result itself.
struct BusyGuard {
    /// Taken on disarm: `None` means there is nothing left to clear.
    srv: Option<Arc<Srv>>,
    league_id: String,
    screen: &'static str,
    device: EntryDevice,
}

impl BusyGuard {
    fn disarm(&mut self) {
        self.srv = None;
    }
}

impl Drop for BusyGuard {
    fn drop(&mut self) {
        let Some(srv) = self.srv.take() else {
            return;
        };
        // No runtime means the app is going down and there is nobody left to
        // tell; spawning there would panic inside a drop.
        let Ok(handle) = tokio::runtime::Handle::try_current() else {
            return;
        };
        let league_id = std::mem::take(&mut self.league_id);
        let screen = self.screen;
        let device = self.device.clone();
        handle.spawn(async move {
            let thread = srv
                .chat
                .finish(
                    &league_id,
                    screen,
                    device,
                    Err("The answer stopped unexpectedly".to_string()),
                )
                .await;
            srv.announce(&thread);
        });
    }
}

#[cfg(test)]
mod tests {
    use super::check_screen;

    #[test]
    fn only_the_two_real_screens_have_a_shared_thread() {
        assert_eq!(check_screen("draft").expect("draft is a screen"), "draft");
        assert_eq!(
            check_screen("season").expect("season is a screen"),
            "season"
        );
        // Anything else would open a thread — and a spend key — under whatever
        // name arrived over the network.
        assert!(check_screen("settings").is_err());
        assert!(check_screen("").is_err());
        assert!(check_screen("../draft").is_err());
    }
}
