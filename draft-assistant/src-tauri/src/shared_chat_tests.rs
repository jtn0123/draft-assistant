//! The thread's own bookkeeping. The wire path — POST, 202, WebSocket entry —
//! is in `tests/companion_wire.rs`.

use super::*;

fn scratch(label: &str) -> PathBuf {
    let dir = std::env::temp_dir().join(format!(
        "draft-assistant-shared-chat-{label}-{}-{}",
        std::process::id(),
        now_ms()
    ));
    std::fs::create_dir_all(&dir).expect("the scratch directory is creatable");
    dir
}

fn phone() -> EntryDevice {
    EntryDevice {
        name: "Rob's iPhone".to_string(),
        kind: "phone".to_string(),
    }
}

fn reply(text: &str, cost: f64) -> ChatReply {
    ChatReply {
        text: text.to_string(),
        thinking: None,
        model: "claude-opus-5".to_string(),
        refused: false,
        truncated: false,
        input_tokens: 10,
        output_tokens: 20,
        cache_creation_input_tokens: 0,
        cache_read_input_tokens: 0,
        provider: "api".to_string(),
        cost_usd: cost,
        screen_spend_usd: cost,
    }
}

#[tokio::test]
async fn a_question_and_its_answer_land_in_the_thread() {
    let chat = SharedChat::new(scratch("answer"));
    let (id, thread) = chat
        .post("league-1", "draft", phone(), "  who should I take? ")
        .await
        .expect("the question is accepted");
    assert!(thread.busy);
    assert_eq!(thread.entries.len(), 1);
    assert_eq!(thread.entries[0].id, id);
    assert_eq!(thread.entries[0].text, "who should I take?");
    assert_eq!(thread.entries[0].role, "user");
    assert_eq!(thread.entries[0].device, phone());

    let thread = chat
        .finish(
            "league-1",
            "draft",
            phone(),
            Ok(reply("Take the RB.", 0.02)),
        )
        .await;
    assert!(!thread.busy);
    assert_eq!(thread.entries.len(), 2);
    assert_eq!(thread.entries[1].role, "assistant");
    assert_eq!(thread.entries[1].cost_usd, Some(0.02));
    assert!(thread.entries[1].error.is_none());
    // The answer carries the asking device, so the phone can tell whose it was.
    assert_eq!(thread.entries[1].device, phone());
}

#[tokio::test]
async fn a_failure_is_an_entry_rather_than_a_dropped_question() {
    let chat = SharedChat::new(scratch("failure"));
    chat.post("l", "draft", phone(), "hi")
        .await
        .expect("posted");
    let thread = chat
        .finish("l", "draft", phone(), Err("no API key".to_string()))
        .await;
    assert!(!thread.busy);
    assert_eq!(thread.entries[1].error.as_deref(), Some("no API key"));
    assert_eq!(thread.entries[1].text, "");
    assert_eq!(thread.entries[1].cost_usd, None);
    // And it is not fed back to the model as if it were an answer.
    let messages = chat.messages("l", "draft").await;
    assert_eq!(messages.len(), 1);
    assert_eq!(messages[0].role, "user");
}

#[tokio::test]
async fn only_one_question_is_in_flight_per_screen() {
    let chat = SharedChat::new(scratch("busy"));
    chat.post("l", "draft", phone(), "one")
        .await
        .expect("the first is accepted");
    assert_eq!(
        chat.post("l", "draft", phone(), "two").await.err(),
        Some(PostError::Busy)
    );
    // The other screen is its own thread and is not blocked by it.
    chat.post("l", "season", phone(), "two")
        .await
        .expect("the season screen is free");
    chat.finish("l", "draft", phone(), Ok(reply("ok", 0.0)))
        .await;
    chat.post("l", "draft", phone(), "three")
        .await
        .expect("the draft screen is free again");
}

#[tokio::test]
async fn empty_and_oversized_questions_are_refused() {
    let chat = SharedChat::new(scratch("bad-text"));
    assert!(matches!(
        chat.post("l", "draft", phone(), "   ").await,
        Err(PostError::BadText(_))
    ));
    let huge = "x".repeat(MAX_QUESTION_BYTES + 1);
    assert!(matches!(
        chat.post("l", "draft", phone(), &huge).await,
        Err(PostError::BadText(_))
    ));
    // Neither left the screen busy.
    assert!(!chat.thread("l", "draft").await.busy);
}

#[tokio::test]
async fn a_thread_is_read_back_off_disk_and_leagues_do_not_share_one() {
    let dir = scratch("persist");
    let chat = SharedChat::new(dir.clone());
    chat.post("league-1", "draft", phone(), "kept")
        .await
        .expect("posted");
    chat.finish("league-1", "draft", phone(), Ok(reply("also kept", 0.01)))
        .await;
    chat.post("league-2", "draft", phone(), "other league")
        .await
        .expect("posted");
    chat.finish("league-2", "draft", phone(), Ok(reply("other", 0.0)))
        .await;

    // A fresh store over the same directory is what a restart looks like.
    let after = SharedChat::new(dir);
    let thread = after.thread("league-1", "draft").await;
    assert_eq!(thread.entries.len(), 2);
    assert_eq!(thread.entries[0].text, "kept");
    assert_eq!(thread.entries[1].text, "also kept");
    // `busy` is in-memory only: a crash mid-answer must not wedge the thread.
    assert!(!thread.busy);
    assert_eq!(after.thread("league-2", "draft").await.entries.len(), 2);
    assert!(after.thread("league-1", "season").await.entries.is_empty());
    assert!(after.thread("nobody", "draft").await.entries.is_empty());
}

#[tokio::test]
async fn a_thread_stops_growing_at_the_cap() {
    let chat = SharedChat::new(scratch("cap"));
    for n in 0..(MAX_ENTRIES + 10) {
        chat.post("l", "draft", phone(), &format!("q{n}"))
            .await
            .expect("posted");
        chat.finish("l", "draft", phone(), Err("no".to_string()))
            .await;
    }
    let thread = chat.thread("l", "draft").await;
    assert_eq!(thread.entries.len(), MAX_ENTRIES);
    // The oldest went, not the newest.
    assert_eq!(
        thread.entries.last().expect("there are entries").role,
        "assistant"
    );
}

#[tokio::test]
async fn a_league_id_that_looks_like_a_path_cannot_escape_the_data_directory() {
    let dir = scratch("escape");
    let chat = SharedChat::new(dir.clone());
    chat.post("../../evil", "draft", phone(), "hi")
        .await
        .expect("posted");
    chat.finish("../../evil", "draft", phone(), Err("no".to_string()))
        .await;
    let written: Vec<_> = std::fs::read_dir(&dir)
        .expect("readable")
        .filter_map(|e| e.ok())
        .map(|e| e.file_name().to_string_lossy().into_owned())
        .collect();
    assert!(
        written.iter().all(|name| !name.contains("..")),
        "{written:?}"
    );
    assert!(dir.join("..").join("..").join("evil").exists().eq(&false));
}

#[tokio::test]
async fn forgetting_what_is_in_memory_still_reads_the_thread_back() {
    let chat = SharedChat::new(scratch("forget"));
    chat.post("l", "draft", phone(), "hi")
        .await
        .expect("posted");
    chat.finish("l", "draft", phone(), Ok(reply("hello", 0.0)))
        .await;
    chat.forget().await;
    assert_eq!(chat.thread("l", "draft").await.entries.len(), 2);
}
