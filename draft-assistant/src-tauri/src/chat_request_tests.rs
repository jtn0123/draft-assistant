//! The request `chat.rs` builds, and the small pure pieces around it.
//!
//! Its own file because `chat.rs` is at the line cap. These are unit tests
//! over the wire *types*: nothing here opens a socket, which is what
//! `chat_wire_tests.rs` next door is for.

use super::*;

#[test]
fn model_ids_are_the_exact_published_strings() {
    assert_eq!(ChatModel::Opus5.id(), "claude-opus-5");
    assert_eq!(ChatModel::Fable5.id(), "claude-fable-5");
}

#[test]
fn effort_labels_map_to_api_values() {
    assert_eq!(Effort::parse("xhigh").api_effort(), "xhigh");
    assert_eq!(Effort::parse("Max").api_effort(), "max");
    assert_eq!(Effort::parse("nonsense").api_effort(), "high");
    // Disabled thinking must not ride at xhigh/max, which the API rejects.
    assert_eq!(Effort::Off.api_effort(), "medium");
}

/// The id an answer reports is dated, and a server-side fallback can answer on
/// a model nobody asked for. Both used to be priced as the request.
#[test]
fn a_reported_model_id_maps_back_to_the_price_list() {
    assert_eq!(
        ChatModel::from_reported("claude-opus-5-20260219"),
        Some(ChatModel::Opus5)
    );
    assert_eq!(
        ChatModel::from_reported("claude-fable-5-20260219"),
        Some(ChatModel::Fable5)
    );
    assert_eq!(
        ChatModel::from_reported("CLAUDE-FABLE-5"),
        Some(ChatModel::Fable5)
    );
    // Nothing recognisable is not guessed at; the caller keeps what it asked
    // for rather than being charged at a made-up rate.
    assert_eq!(ChatModel::from_reported(""), None);
    assert_eq!(ChatModel::from_reported("gpt-9"), None);
}

/// 16,000 output tokens is the documented ceiling for a request that does not
/// stream, and this one does not: a larger one risks the answer outliving the
/// HTTP timeout. So the note has to give the user something they can do, and
/// the effort level is the lever that matters — thinking is billed against
/// the same ceiling the answer is, so a lower effort leaves more of it for
/// the answer.
#[test]
fn the_length_note_says_what_to_do_about_it() {
    assert_eq!(request::MAX_TOKENS, 16000);
    assert!(TRUNCATED_NOTE.contains("cut off"), "{TRUNCATED_NOTE}");
    assert!(TRUNCATED_NOTE.contains("shorter"), "{TRUNCATED_NOTE}");
    assert!(TRUNCATED_NOTE.contains("lower effort"), "{TRUNCATED_NOTE}");
}

#[test]
fn the_length_note_is_appended_only_to_a_truncated_answer() {
    assert_eq!(with_truncation_note("done".into(), false), "done");
    assert_eq!(
        with_truncation_note("cut".into(), true),
        format!("cut\n\n{TRUNCATED_NOTE}")
    );
    assert_eq!(with_truncation_note("  ".into(), true), TRUNCATED_NOTE);
}

fn one_question(text: &str) -> Vec<ChatMessage> {
    vec![ChatMessage {
        role: "user".into(),
        content: text.into(),
    }]
}

#[test]
fn an_empty_key_fails_before_any_request_is_made() {
    let http = reqwest::Client::new();
    let result = tokio_test_block(ask(
        &http,
        "   ",
        ChatModel::Opus5,
        Effort::High,
        &crate::chat_context::draft_split(&crate::chat_fixtures::draft_fixture()),
        &one_question("hi"),
    ));
    assert!(result.unwrap_err().contains("no Anthropic API key"));
}

#[test]
fn the_request_body_matches_the_documented_wire_shape() {
    let messages = one_question("Walker or Bowers?");
    let context = crate::chat_context::draft_split(&crate::chat_fixtures::full_board_fixture());
    let request = build_request(ChatModel::Opus5, Effort::XHigh, &context, &messages);
    let json = serde_json::to_value(&request).unwrap();
    assert_eq!(json["model"], "claude-opus-5");
    assert_eq!(json["max_tokens"], request::MAX_TOKENS);
    assert_eq!(json["output_config"]["effort"], "xhigh");
    assert_eq!(json["thinking"]["type"], "adaptive");
    assert_eq!(json["fallbacks"], "default");
    // budget_tokens is removed on these models and 400s if sent.
    assert!(json["thinking"].get("budget_tokens").is_none());
    assert!(json.get("temperature").is_none());
    // `betas` is what an SDK calls the header field. On the raw wire the
    // beta is a header and only a header; a `betas` key in the body is an
    // unknown parameter, so the fallback opt-in never took effect.
    assert!(json.get("betas").is_none(), "betas belongs in the header");
}

/// Summarised thinking is billed as output tokens and this app has never put
/// it on screen, so every turn paid for a summary that went straight in the
/// bin. The thinking itself is untouched — only the summary is not asked for.
#[test]
fn no_thinking_summary_is_asked_for() {
    let messages = one_question("who?");
    let context = crate::chat_context::draft_split(&crate::chat_fixtures::draft_fixture());
    let json = serde_json::to_value(build_request(
        ChatModel::Opus5,
        Effort::High,
        &context,
        &messages,
    ))
    .unwrap();
    assert_eq!(json["thinking"]["type"], "adaptive");
    assert!(
        json["thinking"].get("display").is_none(),
        "a summary is being paid for: {}",
        json["thinking"]
    );
}

#[test]
fn thinking_is_disabled_only_on_the_model_that_allows_it() {
    let messages = one_question("who?");
    let context = crate::chat_context::draft_split(&crate::chat_fixtures::draft_fixture());
    let off = |model| {
        serde_json::to_value(build_request(model, Effort::Off, &context, &messages)).unwrap()
            ["thinking"]["type"]
            .as_str()
            .expect("a thinking type")
            .to_string()
    };
    assert_eq!(off(ChatModel::Opus5), "disabled");
    // Fable 5 always thinks; asking for it to be off is a 400.
    assert_eq!(off(ChatModel::Fable5), "adaptive");
}

/// The breakpoint used to sit on the block carrying the pick number, so every
/// pick threw the cached prefix away: each question paid the 1.25x write and
/// almost none of them read anything back.
#[test]
fn the_cached_block_does_not_carry_the_pick_number() {
    let view = crate::chat_fixtures::full_board_fixture();
    let context = crate::chat_context::draft_split(&view);
    let blocks = request::system_blocks(&context);
    let breakpoint = blocks
        .iter()
        .position(|b| b.cache_control.is_some())
        .expect("something is cached");
    let prefix: String = blocks[..=breakpoint].iter().map(|b| b.text).collect();
    assert!(!prefix.contains("Now: round"), "{prefix}");
    assert!(!prefix.contains("pick 24"), "{prefix}");
    assert!(!prefix.contains("on the clock"), "{prefix}");
    // And the clock is still sent, after the breakpoint.
    let last = blocks.last().expect("a last block");
    assert!(last.cache_control.is_none());
    assert!(last.text.contains("Now: round 3, pick 24"), "{}", last.text);
}

/// Anthropic stores a cached prefix only once it is long enough — a
/// model-dependent token count, 1,024 on both models this panel offers. At
/// about four characters to the token that is 4,096 characters, which the
/// guidance and a full board clear by around a quarter again. A summariser
/// trimmed too far would quietly stop the caching working at all rather than
/// fail anything, which is what this measures.
#[test]
fn the_cached_prefix_is_long_enough_to_be_cached_at_all() {
    let context = crate::chat_context::draft_split(&crate::chat_fixtures::full_board_fixture());
    let blocks = request::system_blocks(&context);
    let breakpoint = blocks
        .iter()
        .position(|b| b.cache_control.is_some())
        .expect("something is cached");
    let prefix: usize = blocks[..=breakpoint].iter().map(|b| b.text.len()).sum();
    assert!(
        prefix >= 4_096,
        "the cached prefix is {prefix} characters, under the 1,024-token minimum"
    );
}
