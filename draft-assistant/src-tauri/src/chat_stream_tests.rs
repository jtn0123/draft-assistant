//! The stream parser against the hand-written fixture and the edges the
//! fixture cannot show: chunk boundaries, line endings, error events.

use super::*;

/// The recorded-shape fixture under `tests/fixtures/`. Hand-written from the
/// streaming reference, as its own comment lines say; it is not a recording.
const FIXTURE: &str = include_str!("../tests/fixtures/chat_stream.sse");

fn parse(body: &str, chunk: usize) -> Result<Answer, String> {
    let mut stream = Stream::new();
    for piece in body.as_bytes().chunks(chunk) {
        stream.feed(piece)?;
    }
    stream.finish()
}

/// The network hands the body over in whatever pieces it likes; an event cut
/// in half must not be read as two broken ones.
#[test]
fn a_stream_cut_at_arbitrary_byte_boundaries_still_assembles_the_answer() {
    let whole = parse(FIXTURE, FIXTURE.len()).expect("the fixture parses");
    for chunk in [1, 7, 64, 1000] {
        let answer = parse(FIXTURE, chunk).expect("the fixture parses in pieces");
        assert_eq!(answer, whole, "chunk size {chunk}");
    }
    // Text blocks join with a blank line; the thinking block is not in it.
    assert_eq!(whole.text, "Take Bowers.\n\nHe is a tier ahead.");
    assert_eq!(whole.model, "claude-opus-5-20260219");
    assert_eq!(whole.stop_reason.as_deref(), Some("end_turn"));
    assert_eq!(whole.stop_category, None);
    assert_eq!(
        whole.usage,
        Usage {
            input_tokens: 1200,
            output_tokens: 80,
            cache_creation_input_tokens: 300,
            cache_read_input_tokens: 2500,
        }
    );
}

#[test]
fn comment_lines_pings_and_events_nobody_has_heard_of_are_read_past() {
    let body = concat!(
        ": a comment\n",
        "event: message_start\n",
        r#"data: {"type":"message_start","message":{"model":"m","usage":{"input_tokens":3}}}"#,
        "\n\n",
        "event: ping\ndata: {\"type\":\"ping\"}\n\n",
        "event: something_new\ndata: {\"type\":\"something_new\",\"payload\":[1,2]}\n\n",
        "event: content_block_delta\n",
        r#"data: {"type":"content_block_delta","index":0,"delta":{"type":"text_delta","text":"hi"}}"#,
        "\n\n",
    );
    let answer = parse(body, 5).expect("nothing here is an error");
    assert_eq!(answer.text, "hi");
    assert_eq!(answer.usage.input_tokens, 3);
}

#[test]
fn crlf_line_endings_are_accepted() {
    let body = concat!(
        "event: message_start\r\n",
        r#"data: {"type":"message_start","message":{"model":"m","usage":{}}}"#,
        "\r\n\r\n",
        "event: content_block_delta\r\n",
        r#"data: {"type":"content_block_delta","index":0,"delta":{"type":"text_delta","text":"ok"}}"#,
        "\r\n\r\n",
    );
    assert_eq!(parse(body, 3).expect("parses").text, "ok");
}

/// The API's own failure mid-stream arrives as an `error` event on a 200.
#[test]
fn an_error_event_ends_the_stream_with_its_message() {
    let body = concat!(
        r#"data: {"type":"message_start","message":{"model":"m","usage":{}}}"#,
        "\n\n",
        r#"data: {"type":"error","error":{"type":"overloaded_error","message":"Overloaded"}}"#,
        "\n\n",
    );
    let error = parse(body, 1000).unwrap_err();
    assert_eq!(error, "Anthropic API error: Overloaded");
}

#[test]
fn a_body_with_no_message_start_is_not_an_answer() {
    let error = parse("data: {\"type\":\"ping\"}\n\n", 1000).unwrap_err();
    assert!(error.contains("before the answer began"), "{error}");
    let error = parse("{\"content\":\"not a stream\"}", 1000).unwrap_err();
    assert!(
        error.starts_with("unexpected Anthropic stream event"),
        "{error}"
    );
}

/// Which classifier declined travels in `message_delta`'s `stop_details`.
#[test]
fn the_refusal_category_is_read_from_the_message_delta() {
    let body = concat!(
        r#"data: {"type":"message_start","message":{"model":"m","usage":{"input_tokens":9}}}"#,
        "\n\n",
        r#"data: {"type":"message_delta","delta":{"stop_reason":"refusal","stop_details":{"type":"refusal","category":"cyber"}},"usage":{"output_tokens":0}}"#,
        "\n\n",
    );
    let answer = parse(body, 1000).expect("a refusal is a complete stream");
    assert_eq!(answer.stop_reason.as_deref(), Some("refusal"));
    assert_eq!(answer.stop_category.as_deref(), Some("cyber"));
    assert_eq!(answer.text, "");
}

/// `message_delta` may report only the output side. The cache tiers that came
/// with `message_start` are what the turn is billed on and must survive it.
#[test]
fn a_later_usage_report_that_omits_a_count_does_not_zero_it() {
    let body = concat!(
        r#"data: {"type":"message_start","message":{"model":"m","usage":{"input_tokens":10,"cache_read_input_tokens":4000}}}"#,
        "\n\n",
        r#"data: {"type":"message_delta","delta":{"stop_reason":"end_turn"},"usage":{"output_tokens":50}}"#,
        "\n\n",
    );
    let answer = parse(body, 1000).expect("parses");
    assert_eq!(answer.usage.cache_read_input_tokens, 4000);
    assert_eq!(answer.usage.input_tokens, 10);
    assert_eq!(answer.usage.output_tokens, 50);
}

/// What a stream that stopped early had already charged.
#[test]
fn a_stream_that_has_started_reports_the_usage_so_far() {
    let mut stream = Stream::new();
    assert!(!stream.started());
    stream
        .feed(
            concat!(
                r#"data: {"type":"message_start","message":{"model":"m","usage":{"input_tokens":700,"cache_creation_input_tokens":100}}}"#,
                "\n\n"
            )
            .as_bytes(),
        )
        .expect("parses");
    assert!(stream.started());
    assert_eq!(stream.model(), "m");
    assert_eq!(stream.usage().input_tokens, 700);
    assert_eq!(stream.usage().cache_creation_input_tokens, 100);
}
