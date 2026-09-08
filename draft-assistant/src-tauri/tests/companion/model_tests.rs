//! Mobile model choices use the existing authenticated shared-chat permissions.
use crate::harness::host;

#[tokio::test]
async fn model_catalog_is_authenticated_and_exposes_only_choices() {
    let host = host("model-catalog").await;
    assert_eq!(host.get("/api/chat/models", "").await.0, 401);
    let paired = host.pair_ok("Phone", "phone").await;
    let (status, body) = host.get("/api/chat/models", &paired.token).await;
    assert_eq!(status, 200);
    let fields = body.as_object().unwrap();
    assert_eq!(fields.len(), 3);
    assert_eq!(body["default_model"], "Opus 5");
    assert_eq!(body["default_effort"], "High");
    let models = body["models"].as_array().unwrap();
    assert_eq!(models.len(), 4);
    for model in models {
        assert_eq!(model.as_object().unwrap().len(), 4);
        assert!(model["model"].is_string());
        assert!(model["note"].is_string());
        assert_eq!(
            model["available"], false,
            "fixture must not discover any machine CLI"
        );
        assert!(model["efforts"]
            .as_array()
            .unwrap()
            .iter()
            .any(|e| e == "High"));
    }
    assert_eq!(models[0]["available"], false, "fixture API has no key");
    assert_eq!(models[1]["model"], "Fable 5.1");
    assert_eq!(models[1]["note"], "slower, smarter");
    assert_eq!(models[2]["note"], "smarter");
    host.companion.hub.revoke().expect("revoke succeeds");
    assert_eq!(host.get("/api/chat/models", &paired.token).await.0, 401);
}

#[tokio::test]
async fn invalid_selection_never_files_a_question_or_changes_host_settings() {
    let host = host("invalid-model").await;
    let paired = host.pair_ok("Phone", "phone").await;
    for (model, effort) in [
        ("gpt-made-up", "High"),
        ("GPT-6 Astra", "Off"),
        ("Fable 5.1", "Off"),
        ("Opus 5", "infinite"),
    ] {
        let (status, _) = host.post("/api/chat", &paired.token,
            serde_json::json!({"screen":"draft","text":"Who next?","model":model,"effort":effort})).await;
        assert_eq!(status, 400);
    }
    let (_, thread) = host.get("/api/chat?screen=draft", &paired.token).await;
    assert!(thread["entries"].as_array().unwrap().is_empty());
    assert_eq!(thread["busy"], false);
    assert_eq!(
        host.state.config.lock().await.chat_provider.as_deref(),
        Some("api")
    );
    assert_eq!(
        host.post(
            "/api/chat",
            "",
            serde_json::json!({
                "screen":"draft","text":"Who next?","model":"GPT-5.6 Sol","effort":"Low"
            })
        )
        .await
        .0,
        401
    );
}

#[tokio::test]
async fn selected_model_still_obeys_the_host_in_flight_claim() {
    let host = host("selected-model-busy").await;
    let paired = host.pair_ok("Phone", "phone").await;
    let _claim = host.state.chat_claims.reserve("draft.league-1").unwrap();
    let (status, _) = host
        .post(
            "/api/chat",
            &paired.token,
            serde_json::json!({
                "screen":"draft","text":"Who next?","model":"GPT-5.6 Sol","effort":"Low"
            }),
        )
        .await;
    assert_eq!(status, 409);
    let (_, thread) = host.get("/api/chat?screen=draft", &paired.token).await;
    assert!(thread["entries"].as_array().unwrap().is_empty());
}

#[tokio::test]
async fn an_explicit_codex_model_cannot_escape_fixture_isolation() {
    let host = host("isolated-codex").await;
    let paired = host.pair_ok("Phone", "phone").await;
    let (status, _) = host
        .post(
            "/api/chat",
            &paired.token,
            serde_json::json!({
                "screen":"draft", "text":"fixture only", "model":"GPT-5.6 Sol", "effort":"Low"
            }),
        )
        .await;
    assert_eq!(status, 202);
    let answer = tokio::time::timeout(crate::harness::DEADLINE, async {
        loop {
            let (_, thread) = host.get("/api/chat?screen=draft", &paired.token).await;
            if thread["busy"] == false {
                break thread;
            }
            tokio::time::sleep(std::time::Duration::from_millis(20)).await;
        }
    })
    .await
    .expect("isolated model must fail locally without running a CLI");
    assert!(answer["entries"][1]["error"]
        .as_str()
        .unwrap()
        .contains("Codex CLI not found"));
}
