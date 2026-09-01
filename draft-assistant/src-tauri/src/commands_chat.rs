//! Tauri commands for the Ask Claude panel.

use crate::chat::{self, ChatMessage, ChatModel, ChatReply, Effort};
use crate::chat_cli;
use crate::chat_context;
use crate::chat_copy;
use crate::engine::AppConfig;
use crate::state::{season_view_for_chat, view_from, AppState};
use tauri::State;

pub const PROVIDER_API: &str = "api";
pub const PROVIDER_CLI: &str = "claude_code";

#[derive(serde::Serialize)]
pub struct ChatSettings {
    has_key: bool,
    /// Last four characters of the stored key, for confirmation in Settings.
    key_hint: Option<String>,
    /// Whether the Claude Code CLI was found on this machine.
    cli_available: bool,
    /// "api" or "claude_code" — the one answers will go through.
    provider: &'static str,
    /// Where the key is kept: "keychain" or "file".
    key_store: &'static str,
    /// The dollar cap a screen's chat runs under. 0 means the user removed it.
    budget_usd: f64,
    /// screen -> what that screen's chats have cost so far, all conversations
    /// together. This is what the cap is checked against.
    spend_usd: std::collections::HashMap<String, f64>,
    models: Vec<&'static str>,
    /// Effort levels each model accepts — they differ, and sending the wrong
    /// one is a 400.
    efforts: std::collections::HashMap<&'static str, Vec<&'static str>>,
    notes: std::collections::HashMap<&'static str, [&'static str; 2]>,
}

/// Which route a question takes. An explicit choice wins; otherwise the CLI
/// when it is installed and no key has been added, else the API.
pub fn resolve_provider(config: &AppConfig, has_key: bool, cli_available: bool) -> &'static str {
    match config.chat_provider.as_deref() {
        Some(PROVIDER_CLI) if cli_available => PROVIDER_CLI,
        Some(PROVIDER_API) => PROVIDER_API,
        _ if cli_available && !has_key => PROVIDER_CLI,
        _ => PROVIDER_API,
    }
}

/// Store (or clear, with an empty string) the Anthropic API key.
#[tauri::command]
pub async fn set_api_key(state: State<'_, AppState>, key: String) -> Result<bool, String> {
    let trimmed = key.trim().to_string();
    let mut config = state.config.lock().await;
    let next = if trimmed.is_empty() {
        None
    } else {
        Some(trimmed)
    };
    let stored = next.is_some();
    state.engine.store_api_key(&mut config, next).await?;
    Ok(stored)
}

/// Pick "api" or "claude_code" explicitly.
#[tauri::command]
pub async fn set_chat_provider(
    state: State<'_, AppState>,
    provider: String,
) -> Result<&'static str, String> {
    let chosen = match provider.as_str() {
        PROVIDER_API => PROVIDER_API,
        PROVIDER_CLI => PROVIDER_CLI,
        other => return Err(format!("unknown chat provider '{other}'")),
    };
    let mut config = state.config.lock().await;
    config.chat_provider = Some(chosen.to_string());
    state.engine.save_config(&config)?;
    let has_key = state.engine.api_key(&config).await.is_some();
    Ok(resolve_provider(
        &config,
        has_key,
        chat_cli::find_cli().is_some(),
    ))
}

/// What the chat panel needs to render itself before the first message.
#[tauri::command]
pub async fn chat_settings(state: State<'_, AppState>) -> Result<ChatSettings, String> {
    // Copied rather than held: the Keychain lookup below can take a moment
    // the first time, and nothing else should wait on the config for it.
    let config = state.config.lock().await.clone();
    let key = state.engine.api_key(&config).await;
    let key = key.as_deref();
    let cli_available = chat_cli::find_cli().is_some();
    let mut efforts = std::collections::HashMap::new();
    efforts.insert("Opus 5", chat_copy::effort_levels(ChatModel::Opus5));
    efforts.insert("Fable 5", chat_copy::effort_levels(ChatModel::Fable5));
    let mut notes = std::collections::HashMap::new();
    for label in ["Off", "Low", "Medium", "High", "xhigh", "Max"] {
        let (title, foot) = chat_copy::effort_note(Effort::parse(label));
        notes.insert(label, [title, foot]);
    }
    Ok(ChatSettings {
        has_key: key.is_some(),
        key_hint: key.map(chat_copy::mask_key),
        cli_available,
        provider: resolve_provider(&config, key.is_some(), cli_available),
        key_store: if crate::secrets::available() {
            "keychain"
        } else {
            "file"
        },
        budget_usd: budget_of(&config),
        spend_usd: config.chat_spend_usd.clone(),
        models: vec!["Opus 5", "Fable 5"],
        efforts,
        notes,
    })
}

/// The most turns and the most text one question may carry.
const MAX_TURNS: usize = 60;
const MAX_THREAD_BYTES: usize = 200_000;

/// Refuse a thread that is too long to send, with a message the panel can show.
fn check_thread_size(messages: &[ChatMessage]) -> Result<(), String> {
    if messages.len() > MAX_TURNS {
        return Err(format!(
            "this conversation is {} turns long — start a new chat to keep asking",
            messages.len()
        ));
    }
    let bytes: usize = messages.iter().map(|m| m.content.len()).sum();
    if bytes > MAX_THREAD_BYTES {
        return Err(format!(
            "this conversation is too long to send ({} KB) — start a new chat",
            bytes / 1024
        ));
    }
    Ok(())
}

/// The cap a screen's chat runs under until the user sets one of their own.
pub const DEFAULT_BUDGET_USD: f64 = 5.0;

/// The cap in force, in dollars. Zero means the user turned it off.
pub fn budget_of(config: &AppConfig) -> f64 {
    config
        .chat_budget_usd
        .unwrap_or(DEFAULT_BUDGET_USD)
        .max(0.0)
}

/// Refuse a turn whose screen has already spent its cap.
///
/// The check is necessarily *before* the turn, because what a turn costs is
/// only known once it has been answered. So a turn that starts under the cap
/// always finishes, however far over it lands — and then counts in full
/// against the cap, which the next turn is refused by. The cap is a stop, not
/// a ceiling; the overshoot is one turn wide.
fn check_budget(spent: f64, cap: f64, screen: &str) -> Result<(), String> {
    if cap > 0.0 && spent >= cap {
        return Err(format!(
            "Ask Claude has spent ${spent:.2} of its ${cap:.2} cap on the {screen} screen — raise the budget above to keep asking."
        ));
    }
    Ok(())
}

/// Set the dollar cap a screen's chat runs under. Zero turns it off.
#[tauri::command]
pub async fn set_chat_budget(state: State<'_, AppState>, dollars: f64) -> Result<f64, String> {
    let dollars = if dollars.is_finite() && dollars > 0.0 {
        dollars
    } else {
        0.0
    };
    let mut config = state.config.lock().await;
    config.chat_budget_usd = Some(dollars);
    state.engine.save_config(&config)?;
    Ok(dollars)
}

/// Ask Claude about the board or the week. `screen` selects which view is
/// summarised into the system prompt.
#[tauri::command]
pub async fn ask_claude(
    state: State<'_, AppState>,
    screen: String,
    model: String,
    effort: String,
    messages: Vec<ChatMessage>,
) -> Result<ChatReply, String> {
    // The whole thread is forwarded to Anthropic or written to the CLI's
    // stdin, so it is bounded here rather than discovered as a bill or a
    // rejected request.
    check_thread_size(&messages)?;
    let cli = chat_cli::find_cli();
    let config = state.config.lock().await.clone();
    let api_key = state.engine.api_key(&config).await;
    let provider = resolve_provider(&config, api_key.is_some(), cli.is_some());
    // The cap is enforced here rather than in the panel, which cannot be the
    // authority on money: it knows only the conversation in front of it, and
    // it prices turns it did not pay for.
    let spent = config.chat_spend_usd.get(&screen).copied().unwrap_or(0.0);
    check_budget(spent, budget_of(&config), &screen)?;

    // Building a season view is seconds of arithmetic. It must not happen with
    // the pollers' mutexes held, so the season screen's own view is reused and
    // any build that is unavoidable runs off the runtime thread.
    let context = if screen == "season" {
        let view = season_view_for_chat(
            &state.loaded,
            &state.season,
            &state.config,
            &state.last_season_view,
        )
        .await?;
        chat_context::season_context(&view)
    } else {
        let loaded = state.loaded.lock().await;
        let loaded = loaded.as_ref().ok_or("no league loaded")?;
        let config = state.config.lock().await;
        chat_context::draft_context(&view_from(loaded, &config))
    };

    let model = ChatModel::parse(&model);
    let effort = Effort::parse(&effort);
    let mut reply = if provider == PROVIDER_CLI {
        let cli = cli.ok_or("Claude Code CLI not found — install it or add an API key")?;
        chat_cli::ask(&cli, model, effort, &context, &messages).await?
    } else {
        let api_key = api_key.ok_or("no Anthropic API key set — add one in Settings")?;
        chat::ask(
            &state.engine.client.http_client(),
            &api_key,
            model,
            effort,
            &context,
            &messages,
        )
        .await?
    };

    // The CLI route is paid for by a subscription, not by the token: charging
    // it list rates would stop the panel over money nobody spent.
    reply.cost_usd = if provider == PROVIDER_CLI {
        0.0
    } else {
        chat::turn_cost(model, reply.input_tokens, reply.output_tokens)
    };
    reply.provider = provider.to_string();
    let mut config = state.config.lock().await;
    let running = config.chat_spend_usd.entry(screen).or_insert(0.0);
    *running += reply.cost_usd;
    reply.screen_spend_usd = *running;
    // A failure to write it down is not a reason to withhold the answer the
    // user already paid for; the next turn re-reads whatever did land.
    if let Err(e) = state.engine.save_config(&config) {
        eprintln!("could not record what Ask Claude spent: {e}");
    }
    Ok(reply)
}

/// Suggested prompts for the current screen.
#[tauri::command]
pub fn chat_suggestions(screen: String) -> Vec<String> {
    chat_context::suggestions(&screen)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn config(provider: Option<&str>) -> AppConfig {
        AppConfig {
            chat_provider: provider.map(str::to_string),
            ..AppConfig::default()
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
}
