//! The retry policy and the Cancel button, against the stub server in
//! `chat_wire_stub.rs`. Split from `chat_wire_tests.rs` for the line cap.

use super::wire_stub::*;
use super::*;

// ---------- retries ----------

/// A 429, a 529 or a 5xx used to reach the panel on the first try. The same
/// request is sent again after a pause, and an API that recovers in time
/// answers as if nothing had happened.
#[test]
fn a_rate_limit_that_lifts_is_answered_rather_than_shown() {
    let (url, requests) = stub_sequence(vec![
        Canned::new(429, r#"{"error":{"message":"slow down"}}"#),
        Canned::new(529, r#"{"error":{"message":"Overloaded"}}"#),
        Canned::new(
            200,
            answer("Take Bowers.", "end_turn", r#"{"input_tokens":10}"#, 5),
        ),
    ]);
    let reply = ask_url(&url, &client()).expect("the third attempt is a reply");
    assert_eq!(reply.text, "Take Bowers.");
    assert!(!reply.cancelled);
    // Three requests went out, each with the same body.
    let bodies: Vec<String> = requests.try_iter().map(|(_, body)| body).collect();
    assert_eq!(bodies.len(), 3, "{bodies:?}");
    assert!(bodies.windows(2).all(|pair| pair[0] == pair[1]));
}

/// A 529 has no name in the HTTP library, so it read as "529 <unknown status
/// code>" once the retries were spent.
#[test]
fn an_overload_that_does_not_lift_is_named_rather_than_unknown() {
    let url = stub_repeated(
        529,
        r#"{"type":"error","error":{"type":"overloaded_error","message":"Overloaded"}}"#,
        3,
    );
    let error = ask_url(&url, &client()).unwrap_err();
    assert_eq!(
        error.message,
        "Anthropic is overloaded, try again in a moment: Overloaded (gave up after 3 attempts)"
    );
    assert!(!error.message.contains("unknown"), "{}", error.message);
    assert!(error.partial.is_none(), "nothing was billed");
}

/// The API's own `retry-after` is the pause, not the backoff, when it names
/// one: retrying sooner than it asked is how a rate limit gets longer.
#[test]
fn a_retry_after_header_sets_the_pause() {
    let (url, _) = stub_sequence(vec![
        Canned::new(429, r#"{"error":{"message":"slow down"}}"#).header("Retry-After", "1"),
        Canned::new(200, answer("ok", "end_turn", r#"{"input_tokens":1}"#, 1)),
    ]);
    let started = std::time::Instant::now();
    let reply = ask_url(&url, &client()).expect("the second attempt is a reply");
    assert_eq!(reply.text, "ok");
    assert!(
        started.elapsed() >= std::time::Duration::from_millis(900),
        "retried after {:?}, before the second the API asked for",
        started.elapsed()
    );
}

/// A 4xx that is not a rate limit is the request's own fault, and asking again
/// would only be refused again: one attempt, and the message as it was.
#[test]
fn a_bad_request_is_not_retried() {
    let (url, requests) = stub_sequence(vec![
        Canned::new(
            400,
            r#"{"error":{"message":"messages: roles must alternate"}}"#,
        ),
        Canned::new(200, answer("never", "end_turn", r#"{"input_tokens":1}"#, 1)),
    ]);
    let error = ask_url(&url, &client()).unwrap_err();
    assert_eq!(
        error.message,
        "Anthropic API error 400 Bad Request: messages: roles must alternate"
    );
    std::thread::sleep(std::time::Duration::from_millis(50));
    assert_eq!(requests.try_iter().count(), 1, "the 400 was retried");
}

// ---------- cancelling ----------

/// The Cancel button pulls the signal the claim handed out. What comes back
/// is not an error: the text that had arrived, marked cut short, with the
/// usage the stream had reported so the turn is still counted.
#[test]
fn a_cancelled_answer_keeps_the_text_that_had_arrived() {
    let body = message_start(
        "claude-opus-5",
        r#"{"input_tokens":1500,"cache_read_input_tokens":4000}"#,
    ) + &text_block(0, "Take Bowers, because")
        + &"x".repeat(4000);
    // The stub sends the first half and then holds the socket open.
    let (url, _) = stub_with(200, body, std::time::Duration::from_secs(5), true);
    let cancel = CancelSignal::never();
    let puller = cancel.clone();
    std::thread::spawn(move || {
        std::thread::sleep(std::time::Duration::from_millis(300));
        puller.cancel();
    });
    let started = std::time::Instant::now();
    let reply = ask_url_with(&url, &client(), cancel).expect("a cancel is a reply, not an error");
    assert!(
        started.elapsed() < std::time::Duration::from_secs(4),
        "the cancel did not stop the wait"
    );
    assert!(reply.cancelled);
    assert!(!reply.truncated);
    assert!(!reply.refused);
    assert_eq!(reply.text, "Take Bowers, because");
    assert_eq!(reply.model, "claude-opus-5");
    assert_eq!(reply.input_tokens, 1500);
    assert_eq!(reply.cache_read_input_tokens, 4000);
    assert!(
        turn_cost_of(ChatModel::Opus5, &reply) > 0.0,
        "the cut-short turn is still priced"
    );
}

/// Cancelled during the pause before a retry: nothing was ever billed, and
/// nothing is sent again.
#[test]
fn a_cancel_during_the_retry_pause_stops_the_retry() {
    let (url, requests) = stub_sequence(vec![
        Canned::new(429, "").header("Retry-After", "5"),
        Canned::new(200, answer("never", "end_turn", r#"{"input_tokens":1}"#, 1)),
    ]);
    let cancel = CancelSignal::never();
    let puller = cancel.clone();
    std::thread::spawn(move || {
        std::thread::sleep(std::time::Duration::from_millis(200));
        puller.cancel();
    });
    let started = std::time::Instant::now();
    let reply = ask_url_with(&url, &client(), cancel).expect("a reply marked cancelled");
    assert!(reply.cancelled);
    assert_eq!(reply.text, "");
    assert_eq!(reply.input_tokens, 0);
    assert!(started.elapsed() < std::time::Duration::from_secs(4));
    assert_eq!(requests.try_iter().count(), 1, "the retry went out anyway");
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
    let error = ask_url(&stub_repeated(500, &page, 3), &client())
        .unwrap_err()
        .message;
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
        ask_url(&stub_repeated(429, "slow down please", 3), &client())
            .unwrap_err()
            .message,
        "Rate limited by Anthropic (gave up after 3 attempts)"
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

/// The API can send its "not right now" as an `error` event inside a 200
/// instead of as a status. Before `message_start` nothing has been billed, so
/// that is the same overload the 529 is and gets the same second try.
#[test]
fn an_overload_delivered_inside_a_200_is_retried_like_the_status_it_stands_for() {
    let overloaded =
        event(r#"{"type":"error","error":{"type":"overloaded_error","message":"Overloaded"}}"#);
    let (url, requests) = stub_sequence(vec![
        Canned::new(200, overloaded),
        Canned::new(
            200,
            answer("Take Bowers.", "end_turn", r#"{"input_tokens":10}"#, 5),
        ),
    ]);
    let reply = ask_url(&url, &client()).expect("the second attempt is a reply");
    assert_eq!(reply.text, "Take Bowers.");
    std::thread::sleep(std::time::Duration::from_millis(50));
    assert_eq!(
        requests.try_iter().count(),
        2,
        "the error event was a dead end"
    );
}

/// The same overload that does not lift reads as the 529 it is rather than
/// quoting the API's own one word, which in the panel looks like something
/// Claude said.
#[test]
fn an_overload_inside_a_200_that_does_not_lift_is_named_and_counted() {
    let overloaded = format!(
        "{}{}",
        message_start("claude-opus-5", r#"{"input_tokens":1500}"#),
        event(r#"{"type":"error","error":{"type":"overloaded_error","message":"Overloaded"}}"#)
    );
    // After `message_start` the turn is billed, so this one is not sent again:
    // what it must do is say what happened and carry what it cost.
    let (url, requests) = stub_sequence(vec![Canned::new(200, overloaded)]);
    let error = ask_url(&url, &client()).unwrap_err();
    assert_eq!(
        error.message,
        "Anthropic is overloaded, try again in a moment: Overloaded"
    );
    let partial = error.partial.expect("the tokens that were billed");
    assert_eq!(partial.input_tokens, 1500);
    std::thread::sleep(std::time::Duration::from_millis(50));
    assert_eq!(
        requests.try_iter().count(),
        1,
        "a billed turn was sent again"
    );
}

/// A trailing event that will not parse arrives after `message_start`, which
/// is where the billing begins. It used to come back as a bare message, so the
/// tokens Anthropic had already charged for were never counted against the
/// screen's cap.
#[test]
fn a_malformed_last_event_still_reports_what_had_been_charged() {
    let body = message_start(
        "claude-opus-5",
        r#"{"input_tokens":1500,"cache_read_input_tokens":4000}"#,
    ) + &text_block(0, "Take Bowers")
        // No blank line after it: the body ends mid-event.
        + "data: {\"type\":";
    let error = ask_url(&stub_server(200, body), &client()).unwrap_err();
    assert!(
        error
            .message
            .starts_with("unexpected Anthropic stream event"),
        "{}",
        error.message
    );
    let partial = error.partial.expect("the usage that arrived is reported");
    assert_eq!(partial.input_tokens, 1500);
    assert_eq!(partial.cache_read_input_tokens, 4000);
    assert_eq!(partial.model, "claude-opus-5");
    assert!(turn_cost_of(ChatModel::Opus5, &partial) > 0.0);
}

/// Three attempts of a ten-minute request plus the pauses between them is half
/// an hour, which is longer than anything waiting on the answer will wait. The
/// retries are held to a budget, so a question that has already spent it is
/// given up on rather than started again.
#[test]
fn a_question_that_has_run_out_of_budget_is_not_tried_again() {
    let (url, requests) = stub_sequence(vec![
        Canned::new(429, r#"{"error":{"message":"slow down"}}"#),
        Canned::new(200, answer("never", "end_turn", r#"{"input_tokens":1}"#, 1)),
    ]);
    let spent = super::retry::Limits {
        backoff: std::time::Duration::from_millis(10),
        pauses: std::time::Duration::from_millis(1),
    };
    let error = ask_url_limited(&url, &client(), CancelSignal::never(), spent).unwrap_err();
    assert_eq!(error.message, "Rate limited by Anthropic: slow down");
    std::thread::sleep(std::time::Duration::from_millis(50));
    assert_eq!(requests.try_iter().count(), 1, "the retry went out anyway");
}
