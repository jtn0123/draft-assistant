//! Tauri commands for the Ask Claude panel.

use crate::chat::{self, ChatMessage, ChatModel, ChatReply, Effort};
use crate::chat_cli;
use crate::chat_context;
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
    state.engine.store_api_key(&mut config, next)?;
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
    let has_key = state.engine.api_key(&config).is_some();
    Ok(resolve_provider(
        &config,
        has_key,
        chat_cli::find_cli().is_some(),
    ))
}

/// What the chat panel needs to render itself before the first message.
#[tauri::command]
pub async fn chat_settings(state: State<'_, AppState>) -> Result<ChatSettings, String> {
    let config = state.config.lock().await;
    let key = state.engine.api_key(&config);
    let key = key.as_deref();
    let cli_available = chat_cli::find_cli().is_some();
    let mut efforts = std::collections::HashMap::new();
    efforts.insert("Opus 5", chat::effort_levels(ChatModel::Opus5));
    efforts.insert("Fable 5", chat::effort_levels(ChatModel::Fable5));
    let mut notes = std::collections::HashMap::new();
    for label in ["Off", "Low", "Medium", "High", "xhigh", "Max"] {
        let (title, foot) = chat::effort_note(Effort::parse(label));
        notes.insert(label, [title, foot]);
    }
    Ok(ChatSettings {
        has_key: key.is_some(),
        key_hint: key.map(chat::mask_key),
        cli_available,
        provider: resolve_provider(&config, key.is_some(), cli_available),
        key_store: if crate::secrets::available() {
            "keychain"
        } else {
            "file"
        },
        models: vec!["Opus 5", "Fable 5"],
        efforts,
        notes,
    })
}

/// Ask Claude about the board or the week. `screen` selects which view is
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
    let (provider, api_key) = {
        let config = state.config.lock().await;
        let key = state.engine.api_key(&config);
        (resolve_provider(&config, key.is_some(), cli.is_some()), key)
    };

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
    if provider == PROVIDER_CLI {
        let cli = cli.ok_or("Claude Code CLI not found — install it or add an API key")?;
        return chat_cli::ask(&cli, model, effort, &context, &messages).await;
    }
    let api_key = api_key.ok_or("no Anthropic API key set — add one in Settings")?;
    chat::ask(
        &state.engine.client.http_client(),
        &api_key,
        model,
        effort,
        &context,
        &messages,
    )
    .await
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
}
