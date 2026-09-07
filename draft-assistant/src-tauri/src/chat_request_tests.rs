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

/// The ceiling used to be 16,000, the documented default for a request that
/// does not stream, and at a high effort the thinking alone could use it up:
/// the answer arrived cut off. The request streams now, so the timeout no
/// longer bounds the length, and the ceiling is the streaming default. The
/// note still has to give the user something they can do, and the effort
/// level is the lever that matters — thinking is billed against the same
/// ceiling the answer is, so a lower effort leaves more of it for the answer.
#[test]
fn the_request_streams_with_room_for_thinking_and_the_note_says_what_to_do() {
    assert_eq!(request::MAX_TOKENS, 64000);
    let messages = one_question("who?");
    let context = crate::chat_context::draft_split(&crate::chat_fixtures::draft_fixture());
    let json = serde_json::to_value(build_request(
        ChatModel::Opus5,
        Effort::XHigh,
        &context,
        &messages,
    ))
    .unwrap();
    assert_eq!(
        json["stream"], true,
        "a 64,000-token ceiling needs a stream"
    );
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
        CancelSignal::never(),
    ));
    let error = result.unwrap_err();
    assert!(error.message.contains("no Anthropic API key"));
    assert!(error.partial.is_none(), "nothing was billed");
}

/// The board goes after the thread as a system-role message, which the API
/// only accepts after a user turn; and a thread ending on the assistant's
/// turn is a prefill, which these models reject anyway. Refused here, before
/// a request that would have been a 400 is paid for in latency.
#[test]
fn a_thread_ending_on_the_assistants_turn_is_refused_before_it_is_sent() {
    let messages = vec![
        ChatMessage {
            role: "user".into(),
            content: "Walker?".into(),
        },
        ChatMessage {
            role: "assistant".into(),
            content: "Bowers.".into(),
        },
    ];
    let http = reqwest::Client::new();
    let context = crate::chat_context::draft_split(&crate::chat_fixtures::draft_fixture());
    let call = Call {
        endpoint: "http://127.0.0.1:1/v1/messages",
        http: &http,
        api_key: "sk-ant-test",
        model: ChatModel::Opus5,
        effort: Effort::High,
        context: &context,
        messages: &messages,
    };
    let error = tokio_test_block(ask_at(call, CancelSignal::never(), Duration::ZERO)).unwrap_err();
    assert_eq!(error.message, "the last turn must be a question");
}

#[test]
fn the_request_body_matches_the_documented_wire_shape() {
    let messages = one_question("Walker or Bowers?");
    let context = crate::chat_context::draft_split(&crate::chat_fixtures::full_board_fixture());
    let request = build_request(ChatModel::Opus5, Effort::XHigh, &context, &messages);
    let json = serde_json::to_value(&request).unwrap();
    assert_eq!(json["model"], "claude-opus-5");
    assert_eq!(json["max_tokens"], request::MAX_TOKENS);
    assert_eq!(json["stream"], true);
    // The question, then the board as a system-role message after it.
    let messages = json["messages"].as_array().expect("messages");
    assert_eq!(messages.len(), 2, "{messages:?}");
    assert_eq!(messages[0]["role"], "user");
    assert_eq!(messages[0]["content"][0]["type"], "text");
    assert_eq!(messages[0]["content"][0]["text"], "Walker or Bowers?");
    assert_eq!(messages[1]["role"], "system");
    assert_eq!(messages[1]["content"], context.volatile);
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

/// The cached prefix used to carry the board: the recent picks, the forty
/// rows, the tier alerts and the clock, all of which a single pick rewrites.
/// So the "stable" half was rewritten on every pick, each question paid the
/// 1.25x cache write again, and hardly any of them read anything back.
#[test]
fn two_questions_one_pick_apart_share_a_cached_prefix() {
    let before = crate::chat_fixtures::full_board_fixture();
    let mut after = before.clone();
    // One pick happens: the clock moves, a player leaves the board, the
    // recent picks grow, a tier alert fires, and my roster gains a player.
    after.draft.current_pick += 1;
    after.draft.on_clock_name = Some("Eli".into());
    after.draft.my_next_picks.remove(0);
    let taken = after.available.remove(0);
    after.recent_picks.push(crate::view::RecentPick {
        pick_no: before.draft.current_pick,
        round: before.draft.current_round,
        slot: 7,
        slot_name: Some("Dana".into()),
        player_id: taken.player.player_id.clone(),
        name: taken.player.name.clone(),
        position: taken.player.position.clone(),
        team: None,
    });
    after.tier_alerts.push(crate::view::TierAlert {
        position: "WR".into(),
        tier: 2,
        players_left: 1,
    });
    after
        .my_roster
        .as_mut()
        .expect("a roster")
        .players
        .push(crate::draft::RosterEntry {
            player_id: "p9".into(),
            name: "New Guy".into(),
            position: "WR".into(),
            team: None,
            pick_no: 23,
            round: 3,
            is_keeper: false,
        });

    let messages = one_question("Best value at my next pick?");
    let prefix = |view| {
        let context = crate::chat_context::draft_split(view);
        let json = serde_json::to_value(build_request(
            ChatModel::Opus5,
            Effort::High,
            &context,
            &messages,
        ))
        .unwrap();
        // Everything up to and including the last breakpoint is what the
        // cache is keyed on: the system blocks and the history through the
        // question. The board comes after it.
        let system = serde_json::to_string(&json["system"]).unwrap();
        let history: Vec<&serde_json::Value> = json["messages"]
            .as_array()
            .unwrap()
            .iter()
            .take_while(|m| m["role"] != "system")
            .collect();
        let board = json["messages"].as_array().unwrap().last().unwrap()["content"]
            .as_str()
            .unwrap()
            .to_string();
        (
            format!("{system}{}", serde_json::to_string(&history).unwrap()),
            board,
        )
    };
    let (prefix_before, board_before) = prefix(&before);
    let (prefix_after, board_after) = prefix(&after);
    assert_eq!(
        prefix_before, prefix_after,
        "the cached prefix changed across a pick"
    );
    assert_ne!(
        board_before, board_after,
        "the pick must still reach the model"
    );
    for changed in [
        "Now: round",
        "Recent picks",
        "Best available",
        "Tier alerts",
        "Your roster",
    ] {
        assert!(
            !prefix_before.contains(changed),
            "{changed} is in the cached prefix"
        );
        assert!(
            board_after.contains(changed),
            "{changed} is missing from the board"
        );
    }
    // The system-role board is not itself a cache breakpoint.
    assert!(!board_after.contains("cache_control"));
}

/// Anthropic stores a cached prefix only once it is long enough — 1,024
/// tokens on both models this panel offers, about 4,096 characters. The
/// system prompt is under that on its own, so what reaches the minimum is the
/// breakpoint on the last turn of the history: the cached prefix grows with
/// the thread, not with the board. A breakpoint that drifted onto the board
/// would take the cache back to being rewritten on every pick.
#[test]
fn the_cached_prefix_grows_with_the_thread_rather_than_with_the_board() {
    let context = crate::chat_context::draft_split(&crate::chat_fixtures::full_board_fixture());
    let blocks = request::system_blocks(&context);
    let breakpoint = blocks
        .iter()
        .position(|b| b.cache_control.is_some())
        .expect("something is cached");
    assert_eq!(
        breakpoint,
        blocks.len() - 1,
        "the breakpoint is on the last block"
    );
    let system: usize = blocks.iter().map(|b| b.text.len()).sum();
    assert!(
        system >= 1_500,
        "the guidance has been trimmed to {system} characters"
    );

    let mut thread = one_question("Walker or Bowers?");
    let short = request::wire_messages(&context, &thread);
    let cached = |messages: &[request::WireMessage]| -> Vec<serde_json::Value> {
        let json = serde_json::to_value(messages).unwrap();
        let all = json.as_array().unwrap().clone();
        let last_breakpoint = all
            .iter()
            .rposition(|m| m["content"][0].get("cache_control").is_some())
            .expect("a breakpoint in the thread");
        all[..=last_breakpoint].to_vec()
    };
    let one_turn = cached(&short);
    assert_eq!(one_turn.len(), 1);
    assert_eq!(one_turn[0]["role"], "user");

    thread.push(ChatMessage {
        role: "assistant".into(),
        content: "Bowers. ".repeat(400),
    });
    thread.push(ChatMessage {
        role: "user".into(),
        content: "And at RB?".into(),
    });
    let long = request::wire_messages(&context, &thread);
    let three_turns = cached(&long);
    assert_eq!(
        three_turns.len(),
        3,
        "the whole history is under the breakpoint"
    );
    // The earlier turns are byte-identical between the two requests, which is
    // what lets the second read the first back.
    assert_eq!(
        serde_json::to_string(&three_turns[0]["content"][0]["text"]).unwrap(),
        serde_json::to_string(&one_turn[0]["content"][0]["text"]).unwrap()
    );
    let thread_chars: usize = three_turns
        .iter()
        .map(|m| m["content"][0]["text"].as_str().unwrap().len())
        .sum();
    assert!(system + thread_chars >= 4_096, "{}", system + thread_chars);
    // And the board stays after it, without a breakpoint of its own.
    let board = long.last().unwrap();
    assert_eq!(board.role, "system");
    assert!(serde_json::to_value(board).unwrap()["content"].is_string());
}
