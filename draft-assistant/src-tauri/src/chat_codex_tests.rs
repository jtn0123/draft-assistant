use super::*;

const SUCCESS: &str = r#"{"type":"item.completed","item":{"type":"agent_message","text":"Take Gibbs."}}
{"type":"turn.completed","usage":{"input_tokens":1000,"cached_input_tokens":600,"cache_write_input_tokens":100,"output_tokens":50}}"#;

#[test]
fn usage_subtracts_cached_tokens_once_and_prices_the_requested_model() {
    let reply = parse_output(SUCCESS.as_bytes(), ChatModel::Astra).unwrap();
    assert_eq!(reply.text, "Take Gibbs.");
    assert_eq!(reply.input_tokens, 300);
    assert_eq!(reply.cache_read_input_tokens, 600);
    assert_eq!(reply.cache_creation_input_tokens, 100);
    assert_eq!(reply.output_tokens, 50);
    assert!((crate::chat::turn_cost_of(ChatModel::Astra, &reply) - 0.00735).abs() < 1e-9);
    assert_eq!(reply.model, "gpt-6-astra");
}

#[test]
fn incomplete_or_failed_turns_do_not_masquerade_as_answers() {
    for text in [
        "",
        "not json",
        "{",
        r#"{"type":"turn.completed"}"#,
        r#"{"type":"item.completed","item":{"type":"agent_message","text":"unfinished"}}"#,
    ] {
        assert!(parse_output(text.as_bytes(), ChatModel::Sol).is_err());
    }
    let failed = format!(
        "{SUCCESS}\n{}",
        r#"{"type":"turn.failed","error":{"message":"Model access required"}}"#
    );
    assert!(parse_output(failed.as_bytes(), ChatModel::Sol)
        .unwrap_err()
        .contains("Model access required"));
    assert!(parse_output(&[255], ChatModel::Sol).is_err());
}

#[test]
fn openai_models_round_trip_and_expose_only_supported_efforts() {
    for (label, model) in [
        ("GPT-6 Astra", ChatModel::Astra),
        ("GPT-5.6 Sol", ChatModel::Sol),
    ] {
        assert_eq!(ChatModel::parse(label), model);
        assert_eq!(ChatModel::parse(model.id()), model);
        assert_eq!(ChatModel::from_reported(model.id()), Some(model));
        assert!(model.is_openai());
    }
    assert!(!crate::chat_copy::effort_levels(ChatModel::Astra).contains(&"Off"));
    assert!(crate::chat_copy::effort_levels(ChatModel::Sol).contains(&"Off"));
    assert!((crate::chat::turn_cost(ChatModel::Sol, 1_000_000, 1_000_000) - 24.0).abs() < 1e-9);
}

#[cfg(unix)]
fn fake_cli(scratch: &Scratch, body: &str) -> PathBuf {
    use std::os::unix::fs::PermissionsExt;
    let path = scratch.0.join("codex");
    std::fs::write(&path, format!("#!/bin/sh\n{body}\n")).unwrap();
    std::fs::set_permissions(&path, std::fs::Permissions::from_mode(0o700)).unwrap();
    path
}
fn question() -> Vec<ChatMessage> {
    vec![ChatMessage {
        role: "user".into(),
        content: "Pick for my roster".into(),
    }]
}

#[cfg(unix)]
#[tokio::test]
async fn process_receives_board_over_stdin_and_no_project_tools() {
    let fixture = Scratch::new().unwrap();
    let cli = fake_cli(
        &fixture,
        &format!(
            r#"here=$(dirname "$0")
pwd > "$here/cwd"
printf '%s\n' "$@" > "$here/args"
cat > "$here/prompt"
printf '%s\n' '{SUCCESS}'"#
        ),
    );
    let reply = ask(
        &cli,
        ChatModel::Sol,
        Effort::Off,
        "BOARD DATA",
        &question(),
        CancelSignal::never(),
    )
    .await
    .unwrap();
    assert_eq!(reply.text, "Take Gibbs.");
    let prompt = std::fs::read_to_string(fixture.0.join("prompt")).unwrap();
    assert!(prompt.contains("BOARD DATA") && prompt.contains("Pick for my roster"));
    let args = std::fs::read_to_string(fixture.0.join("args")).unwrap();
    for value in [
        "--ignore-user-config",
        "--ephemeral",
        "read-only",
        "gpt-5.6-sol",
        "approval_policy=\"never\"",
        "web_search=\"disabled\"",
        "features.shell_tool=false",
        "features.plugins=false",
        "features.apps=false",
        "features.hooks=false",
        "model_reasoning_effort=\"none\"",
    ] {
        assert!(args.lines().any(|line| line == value), "missing {value}");
    }
    let cwd = std::fs::read_to_string(fixture.0.join("cwd")).unwrap();
    assert_ne!(Path::new(cwd.trim()), fixture.0);
    assert!(!Path::new(cwd.trim()).exists(), "private workspace removed");
}

#[cfg(unix)]
#[tokio::test]
async fn cancellation_and_timeout_reap_the_child() {
    let fixture = Scratch::new().unwrap();
    let cli = fake_cli(&fixture, "cat > /dev/null\nexec sleep 30");
    let signal = CancelSignal::never();
    let pull = signal.clone();
    tokio::spawn(async move {
        tokio::time::sleep(Duration::from_millis(50)).await;
        pull.cancel();
    });
    let started = std::time::Instant::now();
    let reply = ask(&cli, ChatModel::Astra, Effort::Low, "", &question(), signal)
        .await
        .unwrap();
    assert!(reply.cancelled);
    assert!(started.elapsed() < Duration::from_secs(2));
    let error = ask_within(
        &cli,
        ChatModel::Sol,
        Effort::Low,
        "",
        &question(),
        CancelSignal::never(),
        Duration::from_millis(50),
    )
    .await
    .unwrap_err();
    assert!(error.contains("too long"));
}

#[cfg(unix)]
#[tokio::test]
async fn no_launch_when_cancelled_empty_or_wrong_model_and_failures_are_actionable() {
    let signal = CancelSignal::never();
    signal.cancel();
    assert!(
        ask(
            Path::new("/missing"),
            ChatModel::Sol,
            Effort::Low,
            "",
            &question(),
            signal
        )
        .await
        .unwrap()
        .cancelled
    );
    assert!(ask(
        Path::new("/missing"),
        ChatModel::Sol,
        Effort::Low,
        "",
        &[],
        CancelSignal::never()
    )
    .await
    .is_err());
    assert!(ask(
        Path::new("/missing"),
        ChatModel::Opus5,
        Effort::Low,
        "",
        &question(),
        CancelSignal::never()
    )
    .await
    .is_err());
    assert!(ask(
        Path::new("/missing"),
        ChatModel::Sol,
        Effort::Low,
        "",
        &question(),
        CancelSignal::never()
    )
    .await
    .unwrap_err()
    .contains("codex login"));
    let fixture = Scratch::new().unwrap();
    let cli = fake_cli(
        &fixture,
        &format!("cat > /dev/null\nprintf '%s\\n' '{SUCCESS}'\nexit 1"),
    );
    assert!(ask(
        &cli,
        ChatModel::Sol,
        Effort::Low,
        "",
        &question(),
        CancelSignal::never()
    )
    .await
    .unwrap_err()
    .contains("exited"));
}
