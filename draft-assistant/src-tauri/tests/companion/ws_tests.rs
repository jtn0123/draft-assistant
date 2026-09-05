//! The event socket, driven with a real WebSocket client.

use crate::chat_tests::make_answers_fail;
use crate::harness::host;
use futures_util::{SinkExt, StreamExt};
use serde_json::Value;
use tokio_tungstenite::tungstenite::Message;
use tokio_tungstenite::{connect_async, MaybeTlsStream, WebSocketStream};

type Socket = WebSocketStream<MaybeTlsStream<tokio::net::TcpStream>>;

async fn open(base: &str, token: &str) -> Socket {
    let url = format!(
        "{}/api/events?token={token}",
        base.replace("http://", "ws://")
    );
    let (socket, _) = connect_async(url).await.expect("the socket opens");
    socket
}

/// The next `{type, payload}` off the socket, or a failed test. Everything
/// here is local, so a frame that has not arrived in five seconds is not slow
/// — it is missing.
async fn next_frame(socket: &mut Socket) -> Value {
    let frame = tokio::time::timeout(std::time::Duration::from_secs(5), socket.next())
        .await
        .expect("a frame arrives within five seconds")
        .expect("the socket is still open")
        .expect("the frame is readable");
    match frame {
        Message::Text(text) => serde_json::from_str(&text).expect("the frame is JSON"),
        other => panic!("expected a text frame, got {other:?}"),
    }
}

/// The next frame of a given type, skipping the ones this test is not about.
async fn next_of(socket: &mut Socket, kind: &str) -> Value {
    for _ in 0..20 {
        let frame = next_frame(socket).await;
        if frame["type"] == kind {
            return frame;
        }
    }
    panic!("no '{kind}' frame arrived");
}

#[tokio::test]
async fn an_unpaired_token_is_told_so_and_then_closed() {
    let host = host("ws-auth").await;
    let url = format!(
        "{}/api/events?token=not-a-token",
        host.base.replace("http://", "ws://")
    );
    // A browser learns nothing about a refused handshake, so the host accepts
    // this one and closes it with a code of its own. Without that, a phone
    // holding a token a restarted host has forgotten reconnects for ever and
    // never says why.
    let (mut socket, _) = connect_async(url).await.expect("the handshake is accepted");
    let frame = tokio::time::timeout(std::time::Duration::from_secs(5), socket.next())
        .await
        .expect("a frame arrives")
        .expect("the socket is open")
        .expect("the frame is readable");
    match frame {
        Message::Close(Some(close)) => {
            assert_eq!(u16::from(close.code), 4401, "{close:?}");
            assert_eq!(close.reason.as_str(), "revoked");
        }
        other => panic!("expected a close frame, got {other:?}"),
    }
    // And nothing at all is readable through it.
    let (status, body) = host.get("/api/state", "not-a-token").await;
    assert_eq!(status, 401);
    assert_eq!(body["error"], "not paired");
}

#[tokio::test]
async fn a_new_socket_is_told_the_state_of_the_world() {
    let host = host("ws-open").await;
    let paired = host.pair_ok("Rob's iPhone", "phone").await;
    let mut socket = open(&host.base, &paired.token).await;

    let devices = next_of(&mut socket, "devices").await;
    assert_eq!(devices["payload"][0]["name"], "Rob's iPhone");
    // The board and the week as they stand. Nothing re-reads them after a
    // reconnect, so a phone that lost its socket between picks used to sit on
    // a stale board until something moved.
    let draft = next_of(&mut socket, "draft-updated").await;
    assert_eq!(draft["payload"]["league"]["league_id"], "league-1");
    let season = next_of(&mut socket, "season-updated").await;
    assert_eq!(season["payload"]["league"]["league_id"], "league-1");
    // Both screens' threads, so the phone can switch tabs without a fetch.
    let first = next_of(&mut socket, "shared-chat").await;
    let second = next_of(&mut socket, "shared-chat").await;
    let screens = [
        first["payload"]["screen"]
            .as_str()
            .unwrap_or_default()
            .to_string(),
        second["payload"]["screen"]
            .as_str()
            .unwrap_or_default()
            .to_string(),
    ];
    assert!(screens.contains(&"draft".to_string()), "{screens:?}");
    assert!(screens.contains(&"season".to_string()), "{screens:?}");
}

#[tokio::test]
async fn a_ping_is_answered_with_a_pong() {
    let host = host("ws-ping").await;
    let paired = host.pair_ok("Rob's iPhone", "phone").await;
    let mut socket = open(&host.base, &paired.token).await;
    socket
        .send(Message::Text(r#"{"type":"ping"}"#.into()))
        .await
        .expect("the ping is sent");
    assert_eq!(next_of(&mut socket, "pong").await["type"], "pong");
}

#[tokio::test]
async fn a_connected_device_is_shown_as_connected() {
    let host = host("ws-connected").await;
    let paired = host.pair_ok("Rob's iPhone", "phone").await;
    let mut socket = open(&host.base, &paired.token).await;
    // The socket's own arrival re-broadcasts the device list.
    for _ in 0..20 {
        if host.companion.hub.devices()[0].connected {
            break;
        }
        tokio::time::sleep(std::time::Duration::from_millis(20)).await;
    }
    assert!(host.companion.hub.devices()[0].connected);
    socket.close(None).await.expect("the socket closes");
    for _ in 0..50 {
        if !host.companion.hub.devices()[0].connected {
            break;
        }
        tokio::time::sleep(std::time::Duration::from_millis(20)).await;
    }
    assert!(!host.companion.hub.devices()[0].connected);
}

#[tokio::test]
async fn a_question_and_then_its_answer_arrive_over_the_socket() {
    let host = host("ws-chat").await;
    make_answers_fail(&host).await;
    let paired = host.pair_ok("Rob's iPhone", "phone").await;
    let mut socket = open(&host.base, &paired.token).await;
    // Drain the opening frames so what follows is only the new thread.
    next_of(&mut socket, "devices").await;
    next_of(&mut socket, "shared-chat").await;
    next_of(&mut socket, "shared-chat").await;

    let (status, _) = host
        .post(
            "/api/chat",
            &paired.token,
            serde_json::json!({ "screen": "draft", "text": "who should I take?" }),
        )
        .await;
    assert_eq!(status, 202);

    // The question first, on its own, with the thread marked busy.
    let asked = next_of(&mut socket, "shared-chat").await;
    assert_eq!(asked["payload"]["busy"], true);
    let entries = asked["payload"]["entries"].as_array().expect("entries");
    assert_eq!(entries.len(), 1);
    assert_eq!(entries[0]["text"], "who should I take?");
    assert_eq!(entries[0]["role"], "user");

    // Then the answer — here a failure, which is an entry like any other.
    let answered = next_of(&mut socket, "shared-chat").await;
    assert_eq!(answered["payload"]["busy"], false);
    let entries = answered["payload"]["entries"].as_array().expect("entries");
    assert_eq!(entries.len(), 2);
    assert_eq!(entries[1]["role"], "assistant");
    assert!(entries[1]["error"].is_string(), "{:?}", entries[1]);
    assert!(entries[1]["cost_usd"].is_null());
}

#[tokio::test]
async fn a_reconnecting_socket_is_current_without_asking_for_anything() {
    let host = host("ws-reconnect").await;
    let paired = host.pair_ok("Rob's iPhone", "phone").await;
    let first = open(&host.base, &paired.token).await;
    drop(first);
    // The phone comes back — out of a lift, off a lock screen — and opens a
    // new socket with the same token. What it gets is the world as it is now.
    let mut second = open(&host.base, &paired.token).await;
    let draft = next_of(&mut second, "draft-updated").await;
    assert!(draft["payload"]["available"].is_array(), "{draft}");
}

#[tokio::test]
async fn a_draft_update_reaches_the_paired_devices() {
    let host = host("ws-draft").await;
    let paired = host.pair_ok("Rob's iPhone", "phone").await;
    let mut socket = open(&host.base, &paired.token).await;
    next_of(&mut socket, "devices").await;
    // The board the socket opened with, before anything has moved.
    next_of(&mut socket, "draft-updated").await;
    // The same call the poll loop makes when picks move.
    host.companion
        .hub
        .publish("draft-updated", &serde_json::json!({ "picks": 3 }));
    let frame = next_of(&mut socket, "draft-updated").await;
    assert_eq!(frame["payload"]["picks"], 3);
}

#[tokio::test]
async fn revoking_tells_every_device_and_then_drops_it() {
    let host = host("ws-revoke").await;
    let paired = host.pair_ok("Rob's iPhone", "phone").await;
    let mut socket = open(&host.base, &paired.token).await;
    next_of(&mut socket, "devices").await;

    let before = host.companion.hub.code();
    host.companion.hub.revoke().expect("revoke runs");
    assert_ne!(host.companion.hub.code(), before);

    // Told why, and only then dropped.
    let frame = next_of(&mut socket, "revoked").await;
    assert!(frame["payload"].is_object());
    for _ in 0..50 {
        match tokio::time::timeout(std::time::Duration::from_millis(100), socket.next()).await {
            Ok(None) | Ok(Some(Err(_))) => break,
            Ok(Some(Ok(_))) | Err(_) => continue,
        }
    }
    // The token is worthless from here on, socket or no socket.
    let (status, body) = host.get("/api/state", &paired.token).await;
    assert_eq!(status, 401);
    assert_eq!(body["error"], "not paired");
}
