//! Response parsing for [`crate::chat`], exercised against real bytes off a
//! socket rather than against a hand-made `Stream` value.
//!
//! The stub server and the event builders are in `chat_wire_stub.rs`. The
//! request *shape* is pinned by `the_request_body_matches_the_documented_wire_shape`
//! in `chat_request_tests.rs`; what is here is everything that happens to the
//! reply on the way back, and the request as it looks on the socket.

use super::wire_stub::*;
use super::*;

// ---------- replies ----------

/// The hand-written fixture, off a socket: the answer the panel gets.
#[test]
fn the_recorded_stream_off_the_wire_becomes_a_reply() {
    let reply = ask_stub(200, FIXTURE).expect("a 200 is a reply");
    // Text blocks join with a blank line; the panel splits paragraphs on it.
    assert_eq!(reply.text, "Take Bowers.\n\nHe is a tier ahead.");
    // A summary is no longer asked for, and one that arrives anyway is not
    // kept: nothing renders it, and it was paid for on every turn.
    assert_eq!(reply.thinking, None);
    assert_eq!(reply.model, "claude-opus-5-20260219");
    assert_eq!((reply.input_tokens, reply.output_tokens), (1200, 80));
    assert_eq!(reply.cache_creation_input_tokens, 300);
    assert_eq!(reply.cache_read_input_tokens, 2500);
    assert!(!reply.refused);
    assert!(!reply.truncated);
    // The transport leaves the command layer's own fields blank.
    assert_eq!(reply.provider, "");
    assert_eq!(reply.cost_usd, 0.0);
}

/// A thinking block in the stream is skipped rather than joined into the
/// answer: the text the panel shows is the text blocks and nothing else.
#[test]
fn a_thinking_block_never_leaks_into_the_answer() {
    let body = message_start("claude-opus-5", r#"{"input_tokens":5}"#)
        + &thinking_block(0)
        + &text_block(1, "Take Bowers.")
        + &message_delta("end_turn", r#"{"output_tokens":5}"#);
    let reply = ask_stub(200, body).expect("a 200 is a reply");
    assert_eq!(reply.text, "Take Bowers.");
    assert_eq!(reply.thinking, None);
}

#[test]
fn a_refusal_with_no_text_still_says_something() {
    let body = message_start("claude-opus-5", r#"{"input_tokens":10}"#)
        + &message_delta("refusal", r#"{"output_tokens":0}"#);
    let reply = ask_stub(200, body).expect("a refusal arrives as a 200, not an error");
    assert!(reply.refused);
    assert_eq!(reply.text, "Claude declined to answer that one.");
}

/// `stop_details.category` says which classifier declined. It used to be
/// read past, so every refusal read the same and nobody could tell a
/// cybersecurity trip from anything else.
#[test]
fn a_refusal_names_the_safety_category_the_api_gave() {
    let body = message_start("claude-opus-5", r#"{"input_tokens":10}"#)
        + &event(
            r#"{"type":"message_delta","delta":{"stop_reason":"refusal","stop_details":{"type":"refusal","category":"cyber"}},"usage":{"output_tokens":0}}"#,
        );
    let reply = ask_stub(200, body).expect("a refusal is a 200");
    assert!(reply.refused);
    assert_eq!(
        reply.text,
        "Claude declined to answer that one (safety category: cyber)."
    );
    // The category is informational and can be missing on a refusal.
    assert_eq!(refusal_text(None), "Claude declined to answer that one.");
    assert_eq!(
        refusal_text(Some("  ")),
        "Claude declined to answer that one."
    );
}

/// `stop_reason: "max_tokens"` used to be read past in silence, so an answer
/// that stopped mid-sentence reached the panel looking like a complete one.
#[test]
fn an_answer_that_hit_the_length_limit_says_so_in_its_own_text() {
    let reply = ask_stub(
        200,
        answer(
            "Take Bowers because he",
            "max_tokens",
            r#"{"input_tokens":10}"#,
            64000,
        ),
    )
    .expect("a truncated answer is still a 200");
    assert!(reply.truncated);
    assert!(!reply.refused);
    // The note is in the text itself, so the phone and the panel both carry it
    // without either of them having to know the flag exists.
    assert_eq!(
        reply.text,
        format!("Take Bowers because he\n\n{TRUNCATED_NOTE}")
    );
}

/// Thinking can use the whole budget and leave no text at all behind.
#[test]
fn a_truncated_answer_with_no_text_is_the_note_on_its_own() {
    let body = message_start("claude-opus-5", r#"{"input_tokens":10}"#)
        + &thinking_block(0)
        + &message_delta("max_tokens", r#"{"output_tokens":64000}"#);
    let reply = ask_stub(200, body).expect("a 200 is a reply");
    assert!(reply.truncated);
    assert_eq!(reply.text, TRUNCATED_NOTE);
}

/// An answer that ended normally must not grow a note it has no business
/// carrying.
#[test]
fn an_ordinary_answer_carries_no_length_note() {
    let reply = ask_stub(
        200,
        answer("Take Bowers.", "end_turn", r#"{"input_tokens":10}"#, 5),
    )
    .expect("a 200 is a reply");
    assert!(!reply.truncated);
    assert_eq!(reply.text, "Take Bowers.");
}

// ---------- errors ----------

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
/// something Claude said; the status is what the user can act on. What goes
/// to the log is a short, redacted sample and the size — not the page, which
/// a proxy can fill with the request it was forwarding, key and all.
#[test]
fn an_error_body_that_is_not_json_is_logged_short_and_redacted_rather_than_shown() {
    let capture = crate::applog::Capture::start();
    let page = format!(
        "<html>gateway x-api-key: sk-ant-secret-value {}</html>",
        "padding ".repeat(100)
    );
    let error = ask_stub(500, page.clone()).unwrap_err();
    assert!(error.starts_with("Anthropic API error 500"), "{error}");
    assert!(!error.contains("html"), "{error}");
    assert!(!error.contains("gateway"), "{error}");
    let logged = capture
        .lines()
        .into_iter()
        .find(|l| l.contains("not an error object"))
        .expect("the status and a sample were logged");
    assert!(
        logged.contains(&format!("({} bytes)", page.len())),
        "{logged}"
    );
    assert!(!logged.contains("sk-ant-secret-value"), "{logged}");
    assert!(
        !logged.contains(&"padding ".repeat(20)),
        "the whole page was logged: {logged}"
    );
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
fn a_body_that_is_not_a_stream_is_named_as_such_rather_than_panicking() {
    let error = ask_stub(200, r#"{"content":"not a list"}"#).unwrap_err();
    assert!(
        error.starts_with("unexpected Anthropic stream event"),
        "{error}"
    );
}

#[test]
fn a_dead_endpoint_reads_as_a_connection_problem() {
    // Port 1 on loopback: bound by nothing, refused immediately.
    let error = ask_url("http://127.0.0.1:1/v1/messages", &client()).unwrap_err();
    assert!(
        error
            .message
            .starts_with("could not reach the Anthropic API"),
        "{}",
        error.message
    );
    assert!(error.partial.is_none(), "nothing was billed");
}

/// A request the API accepted is billed from `message_start` on, whether or
/// not the rest arrives. An answer the client gave up on halfway used to come
/// back as a bare error, so the tokens it had already been charged for were
/// never counted against the cap.
#[test]
fn an_answer_that_stops_early_still_reports_what_had_been_charged() {
    let body = message_start(
        "claude-opus-5",
        r#"{"input_tokens":1500,"cache_read_input_tokens":4000}"#,
    ) + &text_block(0, "Take ")
        + &"x".repeat(2000);
    let (url, _) = stub_with(200, body, std::time::Duration::from_millis(400), true);
    let impatient = reqwest::Client::builder()
        .no_proxy()
        .timeout(std::time::Duration::from_millis(150))
        .build()
        .expect("http client");
    let error = ask_url(&url, &impatient).unwrap_err();
    assert!(error.message.contains("stopped early"), "{}", error.message);
    let partial = error.partial.expect("the usage that arrived is reported");
    assert_eq!(partial.text, "");
    assert_eq!(partial.input_tokens, 1500);
    assert_eq!(partial.cache_read_input_tokens, 4000);
    assert_eq!(partial.model, "claude-opus-5");
    assert!(turn_cost_of(ChatModel::Opus5, &partial) > 0.0);
}

// ---------- money ----------

/// The system prompt carries a `cache_control: ephemeral` breakpoint, so most of a
/// second turn's prompt is billed as a cache read and never shows up in
/// `input_tokens`. Pricing the turn without those tiers undercounted every
/// conversation past its first question.
#[test]
fn cached_prompt_tokens_are_priced_rather_than_counted_as_free() {
    let reply = ask_stub(
        200,
        answer(
            "Take Bowers.",
            "end_turn",
            r#"{"input_tokens":1000,"cache_creation_input_tokens":1000,"cache_read_input_tokens":1000}"#,
            0,
        ),
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

/// A turn that read the whole prompt back from the cache has almost nothing
/// in `input_tokens`; what it cost is the reads, and they must be in the
/// spend or the cap is enforced against a number that is mostly zero.
#[test]
fn a_turn_served_from_the_cache_is_charged_for_its_cache_reads() {
    let reply = ask_stub(
        200,
        answer(
            "Take Bowers.",
            "end_turn",
            r#"{"input_tokens":12,"cache_read_input_tokens":20000}"#,
            0,
        ),
    )
    .expect("a 200 is a reply");
    assert_eq!(reply.cache_read_input_tokens, 20000);
    let full = turn_cost_of(ChatModel::Opus5, &reply);
    let reads_alone = 20000.0 * 5.0 * 0.1 / 1_000_000.0;
    assert!((full - reads_alone - turn_cost(ChatModel::Opus5, 12, 0)).abs() < 1e-12);
    assert!(
        full > 5.0 * reads_alone / 6.0,
        "the reads are most of the bill"
    );
}

/// A response with no cache tiers at all prices exactly as it always did.
#[test]
fn a_turn_that_used_no_cache_costs_what_it_did_before() {
    let reply = ask_stub(
        200,
        answer("ok", "end_turn", r#"{"input_tokens":1000}"#, 100),
    )
    .expect("a 200 is a reply");
    assert_eq!(reply.cache_creation_input_tokens, 0);
    assert_eq!(reply.cache_read_input_tokens, 0);
    let full = turn_cost_of(ChatModel::Opus5, &reply);
    assert!((full - turn_cost(ChatModel::Opus5, 1000, 100)).abs() < 1e-12);
}

// ---------- the request as sent ----------

/// The request the API actually receives, headers and all.
fn sent_request() -> (String, serde_json::Value) {
    let (url, recv) = stub_with(
        200,
        answer("ok", "end_turn", r#"{"input_tokens":1}"#, 1),
        std::time::Duration::ZERO,
        false,
    );
    ask_url(&url, &client()).expect("a 200 is a reply");
    let (head, body) = recv.recv().expect("the stub captured the request");
    (
        head.to_ascii_lowercase(),
        serde_json::from_str(&body).expect("the body is JSON"),
    )
}

/// Server-side fallbacks are opted into with the `anthropic-beta` header. The
/// opt-in used to ride in the body as `betas`, which is what an SDK calls the
/// field it turns into that header — on the raw wire it is an unknown
/// parameter, so every refusal fell through unrescued.
#[test]
fn the_fallback_beta_travels_as_a_header_and_not_in_the_body() {
    let (head, body) = sent_request();
    assert!(
        head.contains(&format!("anthropic-beta: {FALLBACK_BETA}")),
        "{head}"
    );
    assert!(head.contains("anthropic-version: 2023-06-01"), "{head}");
    assert_eq!(body["fallbacks"], "default");
    assert!(body.get("betas").is_none(), "betas is not a body parameter");
}

/// The key must not be logged, echoed, or sent anywhere but its own header.
#[test]
fn the_key_rides_in_x_api_key_and_nowhere_else() {
    let (head, body) = sent_request();
    assert!(head.contains("x-api-key: sk-ant-test"), "{head}");
    assert!(!body.to_string().contains("sk-ant-test"));
}

/// Where the breakpoints sit and where the board goes, on the wire.
///
/// The top-level system prompt is the guidance and the stable half only, with
/// the breakpoint on the stable half; the board follows the conversation as a
/// system-role message, after a second breakpoint on the question. The board
/// used to be in the system prompt, under the breakpoint, so every pick threw
/// the cached prefix away and each question paid the 1.25x write again.
#[test]
fn the_system_prompt_is_cached_and_the_board_follows_the_conversation() {
    let (_, body) = sent_request();
    assert_eq!(body["stream"], true);
    let system = body["system"].as_array().expect("system blocks");
    assert_eq!(system.len(), 2, "{system:?}");
    assert!(
        system[0].get("cache_control").is_none(),
        "one breakpoint covers both blocks"
    );
    assert_eq!(system[1]["text"], "the league");
    assert_eq!(system[1]["cache_control"]["type"], "ephemeral");
    let messages = body["messages"].as_array().expect("messages");
    assert_eq!(messages.len(), 2, "{messages:?}");
    assert_eq!(messages[0]["role"], "user");
    assert_eq!(messages[0]["content"][0]["text"], "Walker or Bowers?");
    assert_eq!(
        messages[0]["content"][0]["cache_control"]["type"],
        "ephemeral"
    );
    // The board is last, after every breakpoint, where rewriting it is free.
    assert_eq!(messages[1]["role"], "system");
    assert_eq!(messages[1]["content"], "the board\n");
}
