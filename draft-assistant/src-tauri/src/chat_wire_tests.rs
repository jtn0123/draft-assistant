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
        // Drain the whole request before answering. The request is not
        // inspected, but it must all have arrived: closing a socket with
        // unread bytes on it makes the kernel send a reset instead of a
        // FIN, and a client still writing its body then sees "connection
        // reset" in place of the status and body this stub meant to serve.
        let mut request = Vec::new();
        let mut chunk = [0u8; 8192];
        loop {
            match socket.read(&mut chunk) {
                Ok(0) | Err(_) => break,
                Ok(n) => request.extend_from_slice(&chunk[..n]),
            }
            if request_is_complete(&request) {
                break;
            }
        }
        let head = format!(
            "HTTP/1.1 {status} X\r\nContent-Type: application/json\r\nContent-Length: {}\r\nConnection: close\r\n\r\n",
            body.len()
        );
        let _ = socket.write_all(head.as_bytes());
        let _ = socket.write_all(body.as_bytes());
        let _ = socket.flush();
        // Half-close: the client reads a clean end of stream, not a reset.
        let _ = socket.shutdown(std::net::Shutdown::Write);
    });
    url
}

/// True once `bytes` holds the request head and as many body bytes as its
/// `Content-Length` promised. A head with no length is complete on its own.
fn request_is_complete(bytes: &[u8]) -> bool {
    let Some(split) = bytes.windows(4).position(|w| w == b"\r\n\r\n") else {
        return false;
    };
    let head = String::from_utf8_lossy(&bytes[..split]);
    let promised = head
        .lines()
        .find_map(|line| {
            let (name, value) = line.split_once(':')?;
            name.eq_ignore_ascii_case("content-length")
                .then(|| value.trim().parse::<usize>().ok())
                .flatten()
        })
        .unwrap_or(0);
    bytes.len() - (split + 4) >= promised
}

/// A client that ignores `HTTP_PROXY`/`HTTPS_PROXY`. The stub server below is
/// on localhost and must be reached directly: whatever proxy the developer's
/// shell exports is not in the business of forwarding to it. (The offline
/// tests in `projections.rs` used to set those variables process-wide, which
/// made these pass or fail depending on which test ran first; they now point
/// their own client at a dead host instead.)
fn client() -> reqwest::Client {
    reqwest::Client::builder()
        .no_proxy()
        .build()
        .expect("http client")
}

fn ask_stub(status: u16, body: &'static str) -> Result<ChatReply, String> {
    let url = stub_server(status, body);
    tokio_test_block(ask_at(
        &url,
        &client(),
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

/// Anything between the app and Anthropic can answer with its own error page.
/// That page used to be pasted into the chat, where a wall of markup reads as
/// something Claude said; the status is what the user can act on.
#[test]
fn an_error_body_that_is_not_json_is_logged_rather_than_shown() {
    let error = ask_stub(500, "<html>gateway</html>").unwrap_err();
    assert!(error.starts_with("Anthropic API error 500"), "{error}");
    assert!(!error.contains("html"), "{error}");
    assert!(!error.contains("gateway"), "{error}");
}

/// The status-specific sentences survive a body that carries no message.
#[test]
fn a_status_still_says_what_went_wrong_without_a_message_to_quote() {
    assert_eq!(
        ask_stub(401, "<html>denied</html>").unwrap_err(),
        "Anthropic rejected the API key"
    );
    assert_eq!(
        ask_stub(429, "slow down please").unwrap_err(),
        "Rate limited by Anthropic"
    );
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
        &client(),
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

/// The guidance block is sent with `cache_control: ephemeral`, so most of a
/// second turn's prompt is billed as a cache read and never shows up in
/// `input_tokens`. Pricing the turn without those tiers undercounted every
/// conversation past its first question.
#[test]
fn cached_prompt_tokens_are_priced_rather_than_counted_as_free() {
    let reply = ask_stub(
        200,
        r#"{"model":"claude-opus-5","stop_reason":"end_turn",
            "content":[{"type":"text","text":"Take Bowers."}],
            "usage":{"input_tokens":1000,"output_tokens":0,
                     "cache_creation_input_tokens":1000,
                     "cache_read_input_tokens":1000}}"#,
    )
    .expect("a 200 is a reply");
    assert_eq!(reply.cache_creation_input_tokens, 1000);
    assert_eq!(reply.cache_read_input_tokens, 1000);

    // At Opus 5's $5/MTok input: 1000 plain, 1000 written at 1.25x, 1000 read
    // at 0.1x — $0.005 + $0.00625 + $0.0005.
    let full = turn_cost_of(ChatModel::Opus5, &reply);
    assert!((full - 0.011_75).abs() < 1e-9, "{full}");
    // The uncached reading of the same turn, which is what was charged before.
    let plain = turn_cost(ChatModel::Opus5, reply.input_tokens, reply.output_tokens);
    assert!(full > plain, "cached tokens must add to the bill");
}

/// A response with no cache tiers at all prices exactly as it always did.
#[test]
fn a_turn_that_used_no_cache_costs_what_it_did_before() {
    let reply = ask_stub(
        200,
        r#"{"model":"claude-opus-5","stop_reason":"end_turn",
            "content":[{"type":"text","text":"ok"}],
            "usage":{"input_tokens":1000,"output_tokens":100}}"#,
    )
    .expect("a 200 is a reply");
    assert_eq!(reply.cache_creation_input_tokens, 0);
    assert_eq!(reply.cache_read_input_tokens, 0);
    let full = turn_cost_of(ChatModel::Opus5, &reply);
    assert!((full - turn_cost(ChatModel::Opus5, 1000, 100)).abs() < 1e-12);
}
