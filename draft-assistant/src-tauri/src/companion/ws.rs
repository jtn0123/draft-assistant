//! The event socket: `GET /api/events?token=…`.
//!
//! One text frame per update, `{ "type": …, "payload": … }`, the same shapes
//! the desktop webview already gets. The token is in the query rather than a
//! header because a browser's `WebSocket` cannot set one.

use super::hub::Device;
use super::server::Srv;
use axum::extract::ws::{Message, WebSocket, WebSocketUpgrade};
use axum::extract::{Query, State};
use axum::response::Response;
use serde::Deserialize;
use std::sync::Arc;
use tokio::sync::broadcast::error::RecvError;

#[derive(Deserialize)]
pub struct TokenQuery {
    #[serde(default)]
    token: String,
}

pub async fn events(
    State(srv): State<Arc<Srv>>,
    Query(query): Query<TokenQuery>,
    upgrade: WebSocketUpgrade,
) -> Response {
    let Some(device) = srv.hub.device_for(&query.token) else {
        // The handshake is accepted and then closed with a code of our own
        // rather than refused: a browser is told nothing at all about a failed
        // WebSocket handshake — no status, no body — so a phone whose token
        // the host has forgotten would retry for ever without ever learning
        // why. [`REVOKED_CLOSE`] is what sends it back to the pairing screen.
        return upgrade.on_upgrade(close_as_revoked);
    };
    // The token is kept for the life of the socket, not just to open it: a
    // re-pair replaces it while the device id stays the same, and this is
    // what lets the hub tell this one connection it is finished.
    let token = query.token;
    upgrade.on_upgrade(move |socket| run(srv, device, token, socket))
}

/// The close code a client reads as "this token is no good any more". Above
/// 4000, which is the range the WebSocket standard leaves to applications.
pub const REVOKED_CLOSE: u16 = 4401;

async fn close_as_revoked(mut socket: WebSocket) {
    let _ = send_revoked(&mut socket).await;
}

async fn send_revoked(socket: &mut WebSocket) -> Result<(), axum::Error> {
    let frame = axum::extract::ws::CloseFrame {
        code: REVOKED_CLOSE,
        reason: "revoked".into(),
    };
    socket.send(Message::Close(Some(frame))).await
}

async fn run(srv: Arc<Srv>, device: Device, token: String, mut socket: WebSocket) {
    let mut events = srv.hub.subscribe();
    let mut closes = srv.hub.subscribe_closes();
    srv.hub.socket_changed(&device.device_id, true);
    // What the client needs before the first update arrives, so a phone that
    // connects mid-draft is not blank until something changes.
    for frame in opening_frames(&srv).await {
        if socket.send(Message::Text(frame.into())).await.is_err() {
            srv.hub.socket_changed(&device.device_id, false);
            return;
        }
    }
    loop {
        tokio::select! {
            // A token this socket authenticated with that has been replaced
            // or revoked. Without this a phone that paired again went on
            // reading the draft over the socket its old token opened.
            dropped = closes.recv() => match dropped {
                Ok(dead) if dead == token => {
                    let _ = send_revoked(&mut socket).await;
                    break;
                }
                Ok(_) | Err(RecvError::Lagged(_)) => continue,
                Err(RecvError::Closed) => break,
            },
            event = events.recv() => match event {
                Ok(frame) => {
                    let revoked = frame_kind(&frame).as_deref() == Some("revoked");
                    if socket.send(Message::Text(frame.into())).await.is_err() {
                        break;
                    }
                    // A revoke drops every socket: the frame goes out first so
                    // the phone can say why it went back to the pairing screen.
                    if revoked && !srv.hub.still_paired(&device.device_id) {
                        break;
                    }
                }
                // A client too slow to keep up misses updates rather than
                // wedging the fan-out for everyone else. The next frame is a
                // whole view, so there is nothing to replay.
                Err(RecvError::Lagged(_)) => continue,
                Err(RecvError::Closed) => break,
            },
            incoming = socket.recv() => match incoming {
                Some(Ok(Message::Text(text))) => {
                    if !answer_client(&srv, &device, &mut socket, text.as_str()).await {
                        break;
                    }
                }
                Some(Ok(Message::Close(_))) | None | Some(Err(_)) => break,
                // Pings and binary frames need no reply of ours; the library
                // answers protocol-level pings on its own.
                Some(Ok(_)) => {}
            },
        }
    }
    srv.hub.socket_changed(&device.device_id, false);
}

/// Handle one client frame. `false` means the socket should close.
async fn answer_client(srv: &Srv, device: &Device, socket: &mut WebSocket, text: &str) -> bool {
    // A revoked device keeps its socket open until it next says something, so
    // the check goes here as well as on the broadcast.
    if !srv.hub.still_paired(&device.device_id) {
        return false;
    }
    let kind = frame_kind(text);
    if kind.as_deref() == Some("ping") {
        // The keep-alive the contract names. Its other job is the check above:
        // a device revoked while it sat idle finds out on its next ping.
        return socket
            .send(Message::Text(r#"{"type":"pong"}"#.into()))
            .await
            .is_ok();
    }
    true
}

/// The state of the world a freshly connected client is given.
///
/// The views are in here as well as the threads because nothing re-reads them
/// after a reconnect: a phone that lost its socket in the lift used to sit on
/// a board minutes out of date until the next pick moved, and a phone that
/// reconnects between picks could sit on one all round.
async fn opening_frames(srv: &Srv) -> Vec<String> {
    // The host's clock, so a phone whose own clock is minutes out does not
    // show a pick timer that is minutes wrong. The page keeps the difference
    // and applies it wherever it counts something down.
    let mut frames = vec![
        frame(
            "hello",
            Ok(serde_json::json!({
                "server_now_ms": super::hub::now_ms(),
                "host_name": srv.hub.host_name(),
            })),
        ),
        frame("devices", serde_json::to_value(srv.hub.devices())),
    ];
    if let Some(view) = draft_view(srv).await {
        frames.push(frame("draft-updated", serde_json::to_value(view)));
    }
    if let Ok(view) = crate::state::season_view_for_chat(
        &srv.state.loaded,
        &srv.state.season,
        &srv.state.config,
        &srv.state.last_season_view,
    )
    .await
    {
        frames.push(frame("season-updated", serde_json::to_value(&*view)));
    }
    if let Ok(league_id) = super::routes_chat::active_league(srv).await {
        for screen in ["draft", "season"] {
            let thread = srv.chat.thread(&league_id, screen).await;
            frames.push(frame("shared-chat", serde_json::to_value(thread)));
        }
    }
    frames
}

/// The same draft view `GET /api/state` answers with, when a league is open.
async fn draft_view(srv: &Srv) -> Option<crate::view_types::DraftView> {
    let loaded = srv.state.loaded.lock().await;
    let loaded = loaded.as_ref()?;
    let config = srv.state.config.lock().await;
    Some(crate::state::view_from(loaded, &config))
}

/// The `type` of a `{ type, payload }` frame, whichever direction it came from.
fn frame_kind(text: &str) -> Option<String> {
    serde_json::from_str::<serde_json::Value>(text).ok()?["type"]
        .as_str()
        .map(str::to_string)
}

fn frame(kind: &str, payload: Result<serde_json::Value, serde_json::Error>) -> String {
    let payload = payload.unwrap_or(serde_json::Value::Null);
    serde_json::json!({ "type": kind, "payload": payload }).to_string()
}

#[cfg(test)]
mod tests {
    use super::frame;

    #[test]
    fn a_frame_type_is_read_from_the_field_and_not_from_the_text() {
        use super::frame_kind;
        assert_eq!(
            frame_kind(r#"{"type":"revoked","payload":{}}"#).as_deref(),
            Some("revoked")
        );
        assert_eq!(frame_kind(r#"{"type":"ping"}"#).as_deref(), Some("ping"));
        // A thread whose text merely mentions the word is not a revoke.
        assert_eq!(
            frame_kind(r#"{"type":"shared-chat","payload":{"text":"revoked"}}"#).as_deref(),
            Some("shared-chat")
        );
        assert_eq!(frame_kind("not json"), None);
        assert_eq!(frame_kind(r#"{"payload":1}"#), None);
    }

    #[test]
    fn a_frame_is_a_type_and_a_payload() {
        let text = frame("devices", serde_json::to_value(Vec::<u8>::new()));
        let value: serde_json::Value = serde_json::from_str(&text).expect("valid JSON");
        assert_eq!(value["type"], "devices");
        assert!(value["payload"].is_array());
    }

    #[test]
    fn a_payload_that_will_not_serialise_still_produces_a_frame() {
        // f64::NAN has no JSON representation. The client hears about the
        // event with a null payload rather than the socket going quiet.
        let text = frame("poll-health", serde_json::to_value(f64::NAN));
        let value: serde_json::Value = serde_json::from_str(&text).expect("valid JSON");
        assert_eq!(value["type"], "poll-health");
        assert!(value["payload"].is_null());
    }
}
