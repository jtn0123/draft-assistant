//! Response parsing for [`crate::chat`], exercised against real bytes off a
//! socket rather than against a hand-made `Response` value.
//!
//! The stub is a one-shot HTTP server built from the standard library: no
//! stub-server crate is pulled in for it, because the server is fifteen lines
//! and these tests are its only caller. The request *shape* is pinned by
//! `the_request_body_matches_the_documented_wire_shape` in `chat.rs`; what is
//! new here is everything that happens to the reply on the way back.

use super::*;

/// Serve `body` with `status` to exactly one request, and return the URL to
/// send it to. The thread ends with the response.
fn stub_server(status: u16, body: &'static str) -> String {
    use std::io::{Read, Write};
    let listener = std::net::TcpListener::bind("127.0.0.1:0").expect("bind");
    let url = format!(
        "http://{}/v1/messages",
        listener.local_addr().expect("addr")
    );
    std::thread::spawn(move || {
        let (mut socket, _) = listener.accept().expect("accept");
        // Read whatever arrived and move on; the request is not inspected.
        let mut scratch = [0u8; 8192];
        let _ = socket.read(&mut scratch);
        let head = format!(
            "HTTP/1.1 {status} X\r\nContent-Type: application/json\r\nContent-Length: {}\r\nConnection: close\r\n\r\n",
            body.len()
        );
        let _ = socket.write_all(head.as_bytes());
        let _ = socket.write_all(body.as_bytes());
        let _ = socket.flush();
    });
    url
}

fn ask_stub(status: u16, body: &'static str) -> Result<ChatReply, String> {
    let url = stub_server(status, body);
    tokio_test_block(ask_at(
        &url,
        &reqwest::Client::new(),
        "sk-ant-test",
        ChatModel::Opus5,
        Effort::High,
        "context",
        &[ChatMessage {
            role: "user".into(),
            content: "Walker or Bowers?".into(),
        }],
    ))
}

#[test]
fn a_real_response_off_the_wire_becomes_a_reply() {
    let reply = ask_stub(
        200,
        r#"{"model":"claude-opus-5","stop_reason":"end_turn",
            "content":[{"type":"thinking","thinking":"weighing tiers"},
                       {"type":"text","text":"Take Bowers."},
                       {"type":"text","text":"He is a tier ahead."}],
            "usage":{"input_tokens":1200,"output_tokens":80}}"#,
    )
    .expect("a 200 is a reply");
    // Text blocks join with a blank line; the panel splits paragraphs on it.
    assert_eq!(reply.text, "Take Bowers.\n\nHe is a tier ahead.");
    assert_eq!(reply.thinking.as_deref(), Some("weighing tiers"));
    assert_eq!(reply.model, "claude-opus-5");
    assert_eq!((reply.input_tokens, reply.output_tokens), (1200, 80));
    assert!(!reply.refused);
    // The transport leaves the command layer's own fields blank.
    assert_eq!(reply.provider, "");
    assert_eq!(reply.cost_usd, 0.0);
}

#[test]
fn empty_thinking_blocks_do_not_become_an_empty_summary() {
    let reply = ask_stub(
        200,
        r#"{"model":"claude-opus-5","stop_reason":"end_turn",
            "content":[{"type":"thinking","thinking":"  "},
                       {"type":"text","text":"Take Bowers."}],
            "usage":{"input_tokens":5,"output_tokens":5}}"#,
    )
    .expect("a 200 is a reply");
    assert_eq!(reply.thinking, None);
}

#[test]
fn a_refusal_with_no_text_still_says_something() {
    let reply = ask_stub(
        200,
        r#"{"model":"claude-opus-5","stop_reason":"refusal","content":[],
            "usage":{"input_tokens":10,"output_tokens":0}}"#,
    )
    .expect("a refusal arrives as a 200, not an error");
    assert!(reply.refused);
    assert_eq!(reply.text, "Claude declined to answer that one.");
}

#[test]
fn an_error_body_is_unwrapped_into_the_message_the_panel_shows() {
    let error = ask_stub(
        401,
        r#"{"type":"error","error":{"message":"invalid x-api-key"}}"#,
    )
    .unwrap_err();
    assert_eq!(error, "Anthropic rejected the API key: invalid x-api-key");

    let error = ask_stub(429, r#"{"error":{"message":"slow down"}}"#).unwrap_err();
    assert_eq!(error, "Rate limited by Anthropic: slow down");
}

#[test]
fn an_error_body_that_is_not_json_still_reaches_the_user() {
    let error = ask_stub(500, "<html>gateway</html>").unwrap_err();
    assert!(error.contains("500"), "{error}");
    assert!(error.contains("gateway"), "{error}");
}

#[test]
fn a_body_that_is_not_a_message_is_named_as_such_rather_than_panicking() {
    let error = ask_stub(200, r#"{"content":"not a list"}"#).unwrap_err();
    assert!(
        error.starts_with("unexpected Anthropic response shape"),
        "{error}"
    );
}

#[test]
fn a_dead_endpoint_reads_as_a_connection_problem() {
    // Port 1 on loopback: bound by nothing, refused immediately.
    let error = tokio_test_block(ask_at(
        "http://127.0.0.1:1/v1/messages",
        &reqwest::Client::new(),
        "sk-ant-test",
        ChatModel::Opus5,
        Effort::High,
        "context",
        &[ChatMessage {
            role: "user".into(),
            content: "hi".into(),
        }],
    ))
    .unwrap_err();
    assert!(
        error.starts_with("could not reach the Anthropic API"),
        "{error}"
    );
}

#[test]
fn list_prices_are_the_published_per_million_rates() {
    assert_eq!(ChatModel::Opus5.price_per_mtok(), (5.0, 25.0));
    assert_eq!(ChatModel::Fable5.price_per_mtok(), (10.0, 50.0));
    // A million in and a million out, at Opus 5's $5 + $25.
    assert!((turn_cost(ChatModel::Opus5, 1_000_000, 1_000_000) - 30.0).abs() < 1e-9);
    assert!((turn_cost(ChatModel::Fable5, 1_000_000, 1_000_000) - 60.0).abs() < 1e-9);
    assert_eq!(turn_cost(ChatModel::Opus5, 0, 0), 0.0);
}
