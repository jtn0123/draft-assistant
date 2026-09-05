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

#[test]
fn the_length_note_is_appended_only_to_a_truncated_answer() {
    assert_eq!(with_truncation_note("done".into(), false), "done");
    assert_eq!(
        with_truncation_note("cut".into(), true),
        format!("cut\n\n{TRUNCATED_NOTE}")
    );
    assert_eq!(with_truncation_note("  ".into(), true), TRUNCATED_NOTE);
}

#[test]
fn an_empty_key_fails_before_any_request_is_made() {
    let http = reqwest::Client::new();
    let result = tokio_test_block(ask(
        &http,
        "   ",
        ChatModel::Opus5,
        Effort::High,
        "context",
        &[ChatMessage {
            role: "user".into(),
            content: "hi".into(),
        }],
    ));
    assert!(result.unwrap_err().contains("no Anthropic API key"));
}

#[test]
fn the_request_body_matches_the_documented_wire_shape() {
    let messages = [ChatMessage {
        role: "user".into(),
        content: "Walker or Bowers?".into(),
    }];
    let request = Request {
        model: ChatModel::Opus5.id(),
        max_tokens: MAX_TOKENS,
        system: vec![SystemBlock {
            kind: "text",
            text: crate::chat_copy::GUIDANCE,
            cache_control: Some(CacheControl { kind: "ephemeral" }),
        }],
        messages: &messages,
        output_config: OutputConfig { effort: "xhigh" },
        thinking: Some(Thinking {
            kind: "adaptive",
            display: Some("summarized"),
        }),
        fallbacks: "default",
    };
    let json = serde_json::to_value(&request).unwrap();
    assert_eq!(json["model"], "claude-opus-5");
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
