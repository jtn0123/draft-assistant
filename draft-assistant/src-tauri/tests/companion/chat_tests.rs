//! The shared chat over HTTP: accepted, refused, and remembered.

use crate::harness::{host, host_over, Host};
use draft_assistant_lib::shared_chat::EntryDevice;

/// Put the fixture over its budget, so a question that reaches the model layer
/// comes back as an error entry without a key, a CLI, or a socket to Anthropic
/// being involved. Deterministic on any machine, including one with a real key
/// in its Keychain.
pub async fn make_answers_fail(host: &Host) {
    let mut config = host.state.config.lock().await;
    config.chat_budget_usd = Some(1.0);
    config
        .chat_spend_usd
        .insert("draft.league-1".to_string(), 99.0);
    config
        .chat_spend_usd
        .insert("season.league-1".to_string(), 99.0);
}

#[tokio::test]
async fn a_posted_question_is_accepted_at_once_and_answered_later() {
    let host = host("chat-post").await;
    make_answers_fail(&host).await;
    let paired = host.pair_ok("Rob's iPhone", "phone").await;
    let (status, body) = host
        .post(
            "/api/chat",
            &paired.token,
            serde_json::json!({ "screen": "draft", "text": "who should I take?" }),
        )
        .await;
    assert_eq!(status, 202);
    let entry_id = body["entry_id"].as_str().expect("an entry id").to_string();

    // The question is in the thread before the answer is.
    let (status, thread) = host.get("/api/chat?screen=draft", &paired.token).await;
    assert_eq!(status, 200);
    assert_eq!(thread["league_id"], "league-1");
    assert_eq!(thread["screen"], "draft");
    assert_eq!(thread["entries"][0]["id"], entry_id.as_str());
    assert_eq!(thread["entries"][0]["role"], "user");
    assert_eq!(thread["entries"][0]["device"]["name"], "Rob's iPhone");

    // And the failure lands as an entry of its own rather than nothing at all.
    let thread = wait_for_answer(&host, &paired.token).await;
    assert_eq!(thread["busy"], false);
    let answer = &thread["entries"][1];
    assert_eq!(answer["role"], "assistant");
    assert!(
        answer["error"].as_str().expect("an error").contains("cap"),
        "{answer}"
    );
    assert_eq!(answer["device"]["name"], "Rob's iPhone");
}

/// Poll the thread until the answer has landed. The answer runs off the
/// request, so there is nothing to await on the HTTP side.
async fn wait_for_answer(host: &Host, token: &str) -> serde_json::Value {
    for _ in 0..100 {
        let (_, thread) = host.get("/api/chat?screen=draft", token).await;
        if thread["entries"].as_array().map(Vec::len).unwrap_or(0) >= 2 {
            return thread;
        }
        tokio::time::sleep(std::time::Duration::from_millis(20)).await;
    }
    panic!("the answer never arrived");
}

#[tokio::test]
async fn a_second_question_is_refused_while_the_first_is_being_answered() {
    let host = host("chat-busy").await;
    let paired = host.pair_ok("Rob's iPhone", "phone").await;
    // Held busy from underneath rather than by racing two HTTP posts: what is
    // under test is the refusal, not how fast the first answer comes back.
    host.companion
        .chat
        .post(
            "league-1",
            "draft",
            EntryDevice {
                name: "Kitchen Mac".to_string(),
                kind: "desktop".to_string(),
            },
            "already asking",
        )
        .await
        .expect("the first question is accepted");
    let (status, body) = host
        .post(
            "/api/chat",
            &paired.token,
            serde_json::json!({ "screen": "draft", "text": "me too" }),
        )
        .await;
    assert_eq!(status, 409);
    assert_eq!(body["error"], "busy");
    // The other screen is a thread of its own and is not blocked.
    let (status, _) = host
        .post(
            "/api/chat",
            &paired.token,
            serde_json::json!({ "screen": "season", "text": "and the week?" }),
        )
        .await;
    assert_eq!(status, 202);
}

#[tokio::test]
async fn a_question_about_a_screen_that_is_not_one_is_refused() {
    let host = host("chat-screen").await;
    let paired = host.pair_ok("Rob's iPhone", "phone").await;
    for screen in ["settings", "", "Draft"] {
        let (status, _) = host
            .post(
                "/api/chat",
                &paired.token,
                serde_json::json!({ "screen": screen, "text": "hi" }),
            )
            .await;
        assert_eq!(status, 400, "{screen}");
    }
    let (status, _) = host.get("/api/chat?screen=nowhere", &paired.token).await;
    assert_eq!(status, 400);
}

#[tokio::test]
async fn a_device_gets_ten_questions_a_minute() {
    let host = host("chat-rate").await;
    make_answers_fail(&host).await;
    let paired = host.pair_ok("Rob's iPhone", "phone").await;
    let mut refusals = 0;
    for n in 0..12 {
        let (status, _) = host
            .post(
                "/api/chat",
                &paired.token,
                serde_json::json!({ "screen": "draft", "text": format!("question {n}") }),
            )
            .await;
        // 409 while an answer is in flight, 429 once the allowance is gone.
        if status == 429 {
            refusals += 1;
        }
        tokio::time::sleep(std::time::Duration::from_millis(5)).await;
    }
    assert!(refusals >= 2, "the per-minute cap never bit");
}

#[tokio::test]
async fn the_thread_is_still_there_after_the_server_has_been_restarted() {
    let host = host("chat-restart").await;
    make_answers_fail(&host).await;
    let paired = host.pair_ok("Rob's iPhone", "phone").await;
    host.post(
        "/api/chat",
        &paired.token,
        serde_json::json!({ "screen": "draft", "text": "remember me" }),
    )
    .await;
    wait_for_answer(&host, &paired.token).await;

    let data_dir = host.data_dir.clone();
    let state = host.state.clone();
    host.companion.stop();
    drop(host);

    // A brand new companion — new code, nothing paired — over the same data
    // directory, which is what the next launch of the app is.
    let restarted = host_over(data_dir, state).await;
    let paired = restarted.pair_ok("Rob's iPhone again", "phone").await;
    let (status, thread) = restarted.get("/api/chat?screen=draft", &paired.token).await;
    assert_eq!(status, 200);
    let entries = thread["entries"].as_array().expect("entries");
    assert_eq!(entries.len(), 2);
    assert_eq!(entries[0]["text"], "remember me");
    assert_eq!(entries[0]["device"]["name"], "Rob's iPhone");
    assert_eq!(thread["busy"], false);
}

#[tokio::test]
async fn the_host_hears_about_the_shared_chat_on_its_own_window() {
    let host = host("chat-webview").await;
    make_answers_fail(&host).await;
    let paired = host.pair_ok("Rob's iPhone", "phone").await;
    host.post(
        "/api/chat",
        &paired.token,
        serde_json::json!({ "screen": "draft", "text": "anyone home?" }),
    )
    .await;
    wait_for_answer(&host, &paired.token).await;
    let kinds = host.emitted_kinds();
    // Pairing tells the settings screen; the question and the answer tell the
    // chat panel. Both go to the desktop as well as to the phone.
    assert!(kinds.iter().any(|k| k == "companion-devices"), "{kinds:?}");
    assert!(
        kinds.iter().filter(|k| *k == "shared-chat").count() >= 2,
        "{kinds:?}"
    );
}

/// A hung model call used to leave `busy` set for good: the flag is in memory
/// and only `finish` clears it, and nothing bounded how long the answering
/// task waited. Every paired device's composer stayed greyed out until the app
/// was restarted.
#[tokio::test]
async fn an_answer_that_never_arrives_times_out_instead_of_wedging_the_thread() {
    let host = host("chat-hang").await;
    let srv = host.companion.srv().expect("the companion is attached");
    let asker = EntryDevice {
        name: "Rob's iPhone".to_string(),
        kind: "phone".to_string(),
    };
    srv.chat
        .post("league-1", "draft", asker.clone(), "who should I take?")
        .await
        .expect("the question is accepted");
    assert!(srv.chat.thread("league-1", "draft").await.busy);

    draft_assistant_lib::companion::routes_chat::finish_within(
        srv.clone(),
        "draft",
        "league-1".to_string(),
        asker,
        std::time::Duration::from_millis(50),
        async {
            tokio::time::sleep(std::time::Duration::from_secs(60)).await;
            Err::<draft_assistant_lib::chat::ChatReply, String>("never reached".to_string())
        },
    )
    .await;

    let thread = srv.chat.thread("league-1", "draft").await;
    assert!(!thread.busy, "the thread is still marked busy");
    assert_eq!(
        thread.entries[1].error.as_deref(),
        Some("Timed out waiting for an answer")
    );
    // And the next question goes through, which is the point of clearing it.
    srv.chat
        .post("league-1", "draft", phone_again(), "and now?")
        .await
        .expect("the thread is free again");
}

/// The same guarantee when the answering task panics rather than hangs.
#[tokio::test]
async fn a_panicking_answer_still_lets_the_next_question_through() {
    let host = host("chat-panic").await;
    let srv = host.companion.srv().expect("the companion is attached");
    srv.chat
        .post("league-1", "draft", phone_again(), "who?")
        .await
        .expect("the question is accepted");

    draft_assistant_lib::companion::routes_chat::finish_within(
        srv.clone(),
        "draft",
        "league-1".to_string(),
        phone_again(),
        std::time::Duration::from_secs(30),
        async { panic!("the answer blew up") },
    )
    .await;

    let thread = srv.chat.thread("league-1", "draft").await;
    assert!(!thread.busy, "a panic left the thread busy");
    assert_eq!(
        thread.entries[1].error.as_deref(),
        Some("The answer stopped unexpectedly")
    );
}

fn phone_again() -> EntryDevice {
    EntryDevice {
        name: "Rob's iPhone".to_string(),
        kind: "phone".to_string(),
    }
}
