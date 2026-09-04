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
        |_| Err("disk full".to_string()),
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

#[test]
fn an_ordinary_conversation_goes_through() {
    let thread: Vec<ChatMessage> = (0..10).map(|i| turn(&format!("question {i}"))).collect();
    assert!(check_thread_size(&thread).is_ok());
}

#[test]
fn too_many_turns_is_refused_with_something_the_user_can_act_on() {
    let thread: Vec<ChatMessage> = (0..=MAX_TURNS).map(|_| turn("hi")).collect();
    let error = check_thread_size(&thread).unwrap_err();
    assert!(error.contains("new chat"), "unhelpful: {error}");
}

#[test]
fn one_enormous_turn_is_refused_even_though_the_count_is_small() {
    let thread = vec![turn(&"x".repeat(MAX_THREAD_BYTES + 1))];
    assert!(check_thread_size(&thread).is_err());
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
    assert!(error.contains("raise the budget"), "{error}");
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
