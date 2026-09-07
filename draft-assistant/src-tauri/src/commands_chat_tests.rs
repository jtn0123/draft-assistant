//! Tests for the Ask Claude commands: which route answers, what a thread may
//! carry, what the spend cap does, and where the running spend is filed.
//!
//! Its own file because `commands_chat.rs` is at the line cap.

use super::*;

fn config(provider: Option<&str>) -> AppConfig {
    AppConfig {
        chat_provider: provider.map(str::to_string),
        ..AppConfig::default()
    }
}

#[tokio::test]
async fn the_config_is_not_held_while_the_key_goes_to_the_keychain() {
    let config = Mutex::new(AppConfig::default());
    let free = std::cell::Cell::new(false);
    store_key_unlocked(
        &config,
        || async {
            // Standing in for the `security` subprocess: whatever runs
            // here must not be running behind the config mutex.
            free.set(config.try_lock().is_ok());
            Ok(Some("sk-test".to_string()))
        },
        |_| Ok(()),
    )
    .await
    .expect("the key was stored");

    assert!(free.get(), "the Keychain ran with the config mutex held");
    assert_eq!(
        config.lock().await.anthropic_api_key.as_deref(),
        Some("sk-test"),
        "the key was not committed once it was safely stored"
    );
}

/// The Keychain can take seconds and can put a prompt in front of the
/// user. Whatever the pollers wrote in that window used to be rolled back:
/// the key path saved a clone of the config taken *before* the wait.
#[tokio::test]
async fn work_done_while_the_keychain_was_busy_is_not_rolled_back() {
    let config = Mutex::new(AppConfig::default());
    let saved: std::cell::RefCell<Option<AppConfig>> = std::cell::RefCell::new(None);
    store_key_unlocked(
        &config,
        || async {
            // A poll tick lands mid-Keychain and records what a chat spent.
            config
                .lock()
                .await
                .chat_spend_usd
                .insert("draft.42".to_string(), 1.25);
            Ok(Some("sk-test".to_string()))
        },
        |next| {
            *saved.borrow_mut() = Some(next.clone());
            Ok(())
        },
    )
    .await
    .expect("the key was stored");

    let config = config.lock().await;
    assert_eq!(config.anthropic_api_key.as_deref(), Some("sk-test"));
    assert_eq!(config.chat_spend_usd.get("draft.42"), Some(&1.25));
    // And what went to disk is that same config, not the pre-wait copy.
    let saved = saved.borrow();
    let saved = saved.as_ref().expect("the config was written");
    assert_eq!(saved.chat_spend_usd.get("draft.42"), Some(&1.25));
    assert_eq!(saved.anthropic_api_key.as_deref(), Some("sk-test"));
}

/// A failed save must not leave memory claiming a key the file has not
/// got — the clone-save-commit order the rest of the app uses.
#[tokio::test]
async fn a_failed_save_leaves_the_config_alone() {
    let config = Mutex::new(AppConfig::default());
    let error = store_key_unlocked(
        &config,
        || async { Ok(Some("sk-test".to_string())) },
        |_| Err::<(), String>("disk full".to_string()),
    )
    .await
    .unwrap_err();
    assert_eq!(error, "disk full");
    assert!(config.lock().await.anthropic_api_key.is_none());
}

#[test]
fn only_the_two_real_screens_can_ask_a_question() {
    assert!(check_screen("draft").is_ok());
    assert!(check_screen("season").is_ok());
    // Anything else used to open its own uncapped spending tally.
    for junk in ["", "Draft", "../etc", "settings"] {
        assert!(check_screen(junk).is_err(), "{junk:?} was accepted");
    }
}

#[test]
fn the_cli_is_preferred_only_when_there_is_no_key() {
    assert_eq!(resolve_provider(&config(None), false, true), PROVIDER_CLI);
    assert_eq!(resolve_provider(&config(None), true, true), PROVIDER_API);
    assert_eq!(resolve_provider(&config(None), false, false), PROVIDER_API);
}

#[test]
fn an_explicit_choice_wins_unless_the_cli_is_missing() {
    assert_eq!(
        resolve_provider(&config(Some(PROVIDER_CLI)), true, true),
        PROVIDER_CLI
    );
    assert_eq!(
        resolve_provider(&config(Some(PROVIDER_CLI)), false, false),
        PROVIDER_API
    );
    assert_eq!(
        resolve_provider(&config(Some(PROVIDER_API)), false, true),
        PROVIDER_API
    );
}

fn turn(content: &str) -> ChatMessage {
    ChatMessage {
        role: "user".into(),
        content: content.into(),
    }
}

fn answer_turn(content: &str) -> ChatMessage {
    ChatMessage {
        role: "assistant".into(),
        content: content.into(),
    }
}

#[test]
fn an_ordinary_conversation_goes_through_whole() {
    let thread: Vec<ChatMessage> = (0..10).map(|i| turn(&format!("question {i}"))).collect();
    assert_eq!(window(&thread).expect("it fits").len(), thread.len());
}

/// A thread past the limit used to be refused, and the shared thread has no
/// way to start a new one from a phone: the two hundredth question on draft
/// night could never be asked. The end of the thread is sent instead.
#[test]
fn a_thread_past_the_limit_sends_its_tail_rather_than_being_refused() {
    let mut thread: Vec<ChatMessage> = Vec::new();
    for i in 0..MAX_TURNS {
        thread.push(turn(&format!("question {i}")));
        thread.push(answer_turn(&format!("answer {i}")));
    }
    let sent = window(&thread).expect("the tail is sendable");
    assert!(sent.len() <= MAX_TURNS, "{} turns sent", sent.len());
    // The window opens on a question and ends on the newest turn there is.
    assert_eq!(sent[0].role, "user");
    assert_eq!(
        sent.last().expect("a last turn").content,
        thread.last().expect("a last turn").content
    );
}

/// Trimming by count alone still sent the bytes: a window of sixty pasted
/// rosters is over the request limit however few turns it is.
#[test]
fn a_window_that_is_still_too_many_bytes_keeps_trimming() {
    let big = "x".repeat(MAX_THREAD_BYTES / 4);
    let mut thread: Vec<ChatMessage> = Vec::new();
    for _ in 0..10 {
        thread.push(turn(&big));
        thread.push(answer_turn(&big));
    }
    let sent = window(&thread).expect("something fits");
    let bytes: usize = sent.iter().map(|m| m.content.len()).sum();
    assert!(bytes <= MAX_THREAD_BYTES, "{bytes} bytes sent");
    assert_eq!(sent[0].role, "user");
}

#[test]
fn one_enormous_turn_is_refused_even_though_the_count_is_small() {
    let thread = vec![turn(&"x".repeat(MAX_THREAD_BYTES + 1))];
    let error = window(&thread).unwrap_err();
    assert!(error.contains("shorter"), "unhelpful: {error}");
}

/// The API refuses a conversation that opens on an assistant turn, which is
/// exactly what trimming from the front produces every other time.
#[test]
fn the_window_never_opens_on_an_answer() {
    let mut thread: Vec<ChatMessage> = Vec::new();
    for i in 0..MAX_TURNS {
        thread.push(answer_turn(&format!("answer {i}")));
        thread.push(turn(&format!("question {i}")));
    }
    let sent = window(&thread).expect("the tail is sendable");
    assert_eq!(sent[0].role, "user");
}

/// The panel files its conversations and reads its spend figure under the
/// league on screen; the backend used to charge the turn to the league the
/// config was last told about, and the two disagree mid-switch.
#[test]
fn the_turn_is_billed_to_the_league_the_question_is_about() {
    assert_eq!(
        charged_league(Some("loaded"), Some("stale")),
        Some("loaded")
    );
    assert_eq!(
        spend_key("draft", charged_league(Some("loaded"), Some("stale"))),
        "draft.loaded"
    );
    // Nothing loaded yet: the config's league is better than no league at all.
    assert_eq!(charged_league(None, Some("stale")), Some("stale"));
    assert_eq!(charged_league(None, None), None);
}

#[test]
fn a_cap_nobody_has_set_is_the_default_and_zero_means_off() {
    assert_eq!(budget_of(&AppConfig::default()), DEFAULT_BUDGET_USD);
    let off = AppConfig {
        chat_budget_usd: Some(0.0),
        ..AppConfig::default()
    };
    assert_eq!(budget_of(&off), 0.0);
    // A negative cap in a hand-edited config file is not a negative cap.
    let nonsense = AppConfig {
        chat_budget_usd: Some(-3.0),
        ..AppConfig::default()
    };
    assert_eq!(budget_of(&nonsense), 0.0);
}

#[test]
fn the_cap_stops_the_screen_that_reached_it_and_says_which_one() {
    assert!(check_budget(4.99, 5.0, "draft").is_ok());
    let error = check_budget(5.0, 5.0, "draft").unwrap_err();
    assert!(error.contains("$5.00 of its $5.00 cap"), "{error}");
    assert!(error.contains("draft screen"), "{error}");
    assert!(error.contains("Raise the budget"), "{error}");
    // A turn that overshot lands, and the next one is refused for it.
    assert!(check_budget(9.40, 5.0, "season").is_err());
}

#[test]
fn no_cap_never_stops_anything() {
    assert!(check_budget(500.0, 0.0, "draft").is_ok());
}

/// A negative cap used to be rounded up to zero, and zero means *no cap*:
/// typing "-1" into the budget box removed the budget.
#[test]
fn a_negative_budget_is_refused_rather_than_read_as_no_budget() {
    let error = checked_budget(-1.0).unwrap_err();
    assert!(error.contains("cannot be negative"), "{error}");
    assert!(error.contains("turn the cap off"), "{error}");
    assert!(checked_budget(-0.01).is_err());
    assert!(checked_budget(f64::NAN).is_err());
    assert!(checked_budget(f64::INFINITY).is_err());
}

#[test]
fn a_budget_of_zero_or_more_is_accepted_as_typed() {
    // Zero is the deliberate way to turn the cap off, and stays one.
    assert_eq!(checked_budget(0.0).unwrap(), 0.0);
    assert_eq!(checked_budget(12.5).unwrap(), 12.5);
}

/// A fallback answers on a model nobody asked for, and the answer used to be
/// priced as the request: an Opus question answered by Fable was billed at
/// half what it cost, so the cap let turns through on money already spent.
#[test]
fn a_turn_is_priced_as_the_model_that_answered_it() {
    assert_eq!(
        billed_model(ChatModel::Opus5, "claude-fable-5-20260219"),
        ChatModel::Fable5
    );
    assert_eq!(
        billed_model(ChatModel::Fable5, "claude-opus-5-20260219"),
        ChatModel::Opus5
    );
    // An id nothing recognises leaves the requested model in place rather
    // than pricing the turn at a rate that was never published.
    assert_eq!(billed_model(ChatModel::Opus5, ""), ChatModel::Opus5);
    // And the rates really do differ, which is what makes this worth doing.
    assert_ne!(
        ChatModel::Opus5.price_per_mtok(),
        ChatModel::Fable5.price_per_mtok()
    );
}

/// Conversations are filed per screen *and* league; spend was filed per
/// screen alone, so every league drew down one shared cap.
#[test]
fn spend_is_filed_under_the_screen_and_the_league_together() {
    assert_eq!(spend_key("draft", Some("123456")), "draft.123456");
    assert_eq!(spend_key("season", Some("123456")), "season.123456");
    // Two leagues on the same screen keep separate tallies.
    assert_ne!(spend_key("draft", Some("a")), spend_key("draft", Some("b")));
    // And nothing collides with a bare-screen key from the old scheme,
    // which is left in the file and never read.
    for screen in ["draft", "season"] {
        assert_ne!(spend_key(screen, None), screen);
        assert_ne!(spend_key(screen, Some("1")), screen);
    }
}

/// A refused budget used to leave nothing behind once its toast was gone: the
/// panel said no, the user dismissed it, and the log had no record that
/// `set_chat_budget` had even been called. This drives the command's own body
/// — the same macro, the same inner half, the same context.
#[tokio::test]
async fn a_refused_chat_command_leaves_an_error_line_naming_it() {
    let (state, _dir) = AppState::scratch("chat-logging");
    let capture = crate::applog::Capture::start();
    // Nothing is loaded and nothing goes over the network: a negative cap is
    // refused before the config file is ever opened.
    let handed_back: Result<f64, String> = crate::applog::logged!(
        "set_chat_budget",
        ids(&state, "").await,
        set_chat_budget_inner(&state, -1.0).await
    );
    let error = handed_back.expect_err("a negative cap is not a cap");
    assert!(
        error.contains("cannot be negative"),
        "the toast says what it always said: {error}"
    );
    assert!(
        capture.saw(&format!("ERROR set_chat_budget failed: {error}")),
        "{:?}",
        capture.lines()
    );
}

/// A screen name that is not a screen used to get as far as reading the
/// thread: an empty thread was refused as "nothing to ask", which told the
/// caller nothing about the name that was wrong.
#[tokio::test]
async fn a_junk_screen_is_refused_before_the_thread_is_looked_at() {
    let (state, _dir) = AppState::scratch("chat-junk-screen");
    let error = answer(&state, "settings", "", "", Vec::new())
        .await
        .expect_err("not a screen");
    assert!(error.contains("not a screen"), "{error}");
}

fn billed(input_tokens: u32) -> ChatReply {
    ChatReply {
        text: String::new(),
        thinking: None,
        model: "claude-opus-5".into(),
        refused: false,
        truncated: false,
        cancelled: false,
        input_tokens,
        output_tokens: 0,
        cache_creation_input_tokens: 0,
        cache_read_input_tokens: 0,
        provider: String::new(),
        cost_usd: 0.0,
        screen_spend_usd: 0.0,
    }
}

fn books(state: &AppState, key: &str) -> Books {
    Books {
        config: state.config.clone(),
        engine: state.engine.clone(),
        key: key.to_string(),
        model: ChatModel::Opus5,
        provider: PROVIDER_API,
    }
}

async fn spent(state: &AppState, key: &str) -> f64 {
    state
        .config
        .lock()
        .await
        .chat_spend_usd
        .get(key)
        .copied()
        .unwrap_or(0.0)
}

/// The shared thread stops waiting for an answer after its limit and aborts
/// the future. The request does not stop with it: the API bills from the
/// moment it accepts the call. That turn used to be billed, discarded, and
/// never counted — the next question passed a cap it should have failed.
#[tokio::test]
async fn a_turn_abandoned_mid_answer_is_still_counted_against_the_cap() {
    let (state, _dir) = AppState::scratch("chat-abandoned");
    let key = "draft.abandoned";
    let in_flight = state.chat_claims.reserve(key).expect("first claim");
    let call = async {
        tokio::time::sleep(std::time::Duration::from_millis(120)).await;
        // A million input tokens at Opus 5's $5/MTok: five dollars.
        Ok(billed(1_000_000))
    };
    let waiting = tokio::spawn(settle(books(&state, key), in_flight, call));
    tokio::time::sleep(std::time::Duration::from_millis(20)).await;
    waiting.abort();
    assert!(waiting.await.is_err(), "the caller stopped waiting");
    // The caller is gone; the call and its bookkeeping are not.
    assert_eq!(spent(&state, key).await, 0.0, "nothing is billed yet");
    assert!(
        state.chat_claims.reserve(key).is_err(),
        "the claim is held until the call ends, not until the caller leaves"
    );
    tokio::time::sleep(std::time::Duration::from_millis(300)).await;
    assert!((spent(&state, key).await - 5.0).abs() < 1e-9);
    state
        .chat_claims
        .reserve(key)
        .expect("the claim was released when the call ended");
}

/// A call that fails after the API has started answering — the client's own
/// timeout, a dropped socket — was still billed for what had arrived.
#[tokio::test]
async fn a_call_that_failed_after_the_api_started_answering_is_still_counted() {
    let (state, _dir) = AppState::scratch("chat-failed-late");
    let key = "draft.failed-late";
    let in_flight = state.chat_claims.reserve(key).expect("first claim");
    let call = async {
        Err(chat::ChatError {
            message: "the Anthropic answer stopped early: timed out".to_string(),
            partial: Some(Box::new(billed(200_000))),
        })
    };
    let error = settle(books(&state, key), in_flight, call)
        .await
        .expect_err("the failure still reaches the caller");
    assert!(error.contains("stopped early"), "{error}");
    assert!((spent(&state, key).await - 1.0).abs() < 1e-9);
}

/// The ordinary path: the reply comes back priced, labelled, and already
/// written down.
#[tokio::test]
async fn a_reply_is_priced_and_recorded_before_it_is_handed_back() {
    let (state, _dir) = AppState::scratch("chat-recorded");
    let key = "draft.recorded";
    let in_flight = state.chat_claims.reserve(key).expect("first claim");
    let reply = settle(books(&state, key), in_flight, async { Ok(billed(100_000)) })
        .await
        .expect("a reply");
    assert!((reply.cost_usd - 0.5).abs() < 1e-9);
    assert_eq!(reply.provider, PROVIDER_API);
    assert!((reply.screen_spend_usd - 0.5).abs() < 1e-9);
    assert!((spent(&state, key).await - 0.5).abs() < 1e-9);
}

/// The command's own path, on a test-built engine: the key lands in the file
/// store inside the scratch directory and nowhere else. `AppState::scratch`
/// builds its engine with `Engine::new`, whose only store is that file, so
/// no `/usr/bin/security` can run here whatever machine this is on.
#[tokio::test]
async fn set_api_key_writes_the_engines_own_store_and_keeps_it_out_of_the_config() {
    let (state, dir) = AppState::scratch("chat-set-key");
    const KEY: &str = "sk-ant-api03-scratch-only-key";
    assert!(set_api_key_inner(&state, format!("  {KEY} \n"))
        .await
        .expect("stored"));
    let secrets = std::fs::read_to_string(dir.join("yahoo-secrets.json"))
        .expect("the scratch file store was written");
    assert!(secrets.contains(KEY), "the key is not in the file store");
    assert_eq!(
        state
            .engine
            .secret_store()
            .and_then(crate::secrets::load_from)
            .as_deref(),
        Some(KEY),
        "trimmed, and read back through the engine's store"
    );
    let config = state.config.lock().await.clone();
    assert!(
        config.anthropic_api_key.is_none(),
        "the store holds the key, so the config must not"
    );
    let on_disk = std::fs::read_to_string(dir.join("config.json")).unwrap_or_default();
    assert!(!on_disk.contains(KEY), "the key was written to config.json");
    assert_eq!(state.engine.api_key(&config).await.as_deref(), Some(KEY));

    // An empty box clears it from the same place.
    assert!(!set_api_key_inner(&state, "   ".to_string())
        .await
        .expect("cleared"));
    assert!(state
        .engine
        .secret_store()
        .and_then(crate::secrets::load_from)
        .is_none());
    let _ = std::fs::remove_dir_all(&dir);
}
