//! Tauri commands for the Ask Claude panel.

use crate::chat::{self, ChatMessage, ChatModel, ChatReply, Effort};
use crate::chat_cli;
use crate::chat_client;
use crate::chat_context;
use crate::chat_copy;
use crate::engine::AppConfig;
use crate::state::{season_view_for_chat, view_from, AppState};
use tauri::State;
use tokio::sync::Mutex;

const PROVIDER_API: &str = "api";
const PROVIDER_CLI: &str = "claude_code";

#[derive(serde::Serialize)]
pub struct ChatSettings {
    has_key: bool,
    /// The stored key, masked, for confirmation in Settings.
    key_hint: Option<String>,
    /// Whether the Claude Code CLI was found on this machine.
    cli_available: bool,
    /// "api" or "claude_code" — the one answers will go through.
    provider: &'static str,
    /// Where the key is kept: "keychain" or "file".
    key_store: &'static str,
    /// The dollar cap a screen's chat runs under. 0 means the user removed it.
    budget_usd: f64,
    /// `screen.league_id` -> what that screen's chats about that league have
    /// cost so far, all conversations together. This is what the cap is
    /// checked against. Bare-screen keys are from an older scheme and are not
    /// read; see [`spend_key`].
    spend_usd: std::collections::HashMap<String, f64>,
    models: Vec<&'static str>,
    /// Effort levels each model accepts — they differ, and sending the wrong
    /// one is a 400.
    efforts: std::collections::HashMap<&'static str, Vec<&'static str>>,
    notes: std::collections::HashMap<&'static str, [&'static str; 2]>,
}

/// Which route a question takes. An explicit choice wins; otherwise the CLI
/// when it is installed and no key has been added, else the API.
fn resolve_provider(config: &AppConfig, has_key: bool, cli_available: bool) -> &'static str {
    match config.chat_provider.as_deref() {
        Some(PROVIDER_CLI) if cli_available => PROVIDER_CLI,
        Some(PROVIDER_API) => PROVIDER_API,
        _ if cli_available && !has_key => PROVIDER_CLI,
        _ => PROVIDER_API,
    }
}

/// Run `store` with the config mutex free, then commit the one field it
/// decided on — the key the config file should now carry, or `None`.
///
/// The mutex is deliberately not held while `store` runs: storing the key
/// means the `security` command in a subprocess, which can take a moment and
/// can put a prompt in front of the user. Held across that, every command that
/// reads the config — and both poll ticks — waited on the Keychain.
///
/// Committing is the clone-save-commit the rest of the app uses: the live
/// config is re-read *after* the wait, edited on a copy, written, and only
/// then swapped in. Nothing the pollers did while the Keychain was busy is
/// rolled back, and a failed save leaves memory and disk agreeing.
async fn store_key_unlocked<F, Fut, S>(
    config: &Mutex<AppConfig>,
    store: F,
    save: S,
) -> Result<(), String>
where
    F: FnOnce() -> Fut,
    Fut: std::future::Future<Output = Result<Option<String>, String>>,
    S: FnOnce(&AppConfig) -> Result<(), String>,
{
    let in_file = store().await?;
    let mut config = config.lock().await;
    let mut next = config.clone();
    next.anthropic_api_key = in_file;
    save(&next)?;
    *config = next;
    Ok(())
}

/// Store (or clear, with an empty string) the Anthropic API key.
#[tauri::command]
pub async fn set_api_key(state: State<'_, AppState>, key: String) -> Result<bool, String> {
    let trimmed = key.trim().to_string();
    let next = if trimmed.is_empty() {
        None
    } else {
        Some(trimmed)
    };
    let stored = next.is_some();
    let engine = state.engine.clone();
    store_key_unlocked(
        &state.config,
        || async move { engine.store_api_key(next).await },
        |config| state.engine.save_config(config),
    )
    .await?;
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

/// The cap a caller asked for, or a refusal.
///
/// A negative cap used to be quietly rounded up to zero — and zero is the one
/// value that means *no cap at all*, so "-1" turned the budget off instead of
/// being rejected. Nothing below zero is a cap, so nothing below zero is
/// accepted; the panel keeps whatever cap it had and says why.
fn checked_budget(dollars: f64) -> Result<f64, String> {
    if !dollars.is_finite() {
        return Err("that is not a number of dollars".to_string());
    }
    if dollars < 0.0 {
        return Err(format!(
            "a budget cannot be negative (${dollars:.2}) — 0 is the way to turn the cap off"
        ));
    }
    Ok(dollars)
}

/// Set the dollar cap a screen's chat runs under. Zero turns it off.
#[tauri::command]
pub async fn set_chat_budget(state: State<'_, AppState>, dollars: f64) -> Result<f64, String> {
    let dollars = checked_budget(dollars)?;
    let mut config = state.config.lock().await;
    config.chat_budget_usd = Some(dollars);
    state.engine.save_config(&config)?;
    Ok(dollars)
}

/// The two screens that can ask a question.
///
/// `screen` picks the context, and further down it keys the running spend the
/// budget is checked against. Anything else would open a fresh, uncapped tally
/// under whatever name arrived over the IPC — and the config would grow a new
/// entry for every one of them.
fn check_screen(screen: &str) -> Result<(), String> {
    match screen {
        "draft" | "season" => Ok(()),
        other => Err(format!("'{other}' is not a screen Ask Claude answers for")),
    }
}

/// Where one screen's running spend is filed: `screen.league_id`.
///
/// The same shape the panel files its saved conversations under (`chatScope`
/// in `chatSessions.ts`), and for the same reason — a question about one
/// league's board is not a question about another's. Spend used to be keyed by
/// screen alone, so every league on the machine drew down one shared cap and
/// the panel's "spent on this screen" figure belonged to no league in
/// particular.
///
/// Keys written under the old scheme are bare screen names, which no scope can
/// collide with. They are left in the config and never read: they are a
/// mixture of every league's spending, so there is no league to migrate them
/// to, and the alternative — charging them all to whichever league happens to
/// be open — would refuse turns over money that league never spent.
pub fn spend_key(screen: &str, league_id: Option<&str>) -> String {
    format!("{screen}.{}", league_id.unwrap_or("none"))
}

/// What a turn is billed at.
///
/// The requested model is what the panel picked; the reported one is what
/// answered. Those differ whenever a server-side fallback rescues a refusal,
/// and pricing the answer as the request charged the wrong rate — under, if
/// Opus was asked for and Fable answered, so the cap let the next turn
/// through on money that had already been spent.
fn billed_model(requested: ChatModel, reported: &str) -> ChatModel {
    ChatModel::from_reported(reported).unwrap_or(requested)
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
    answer(&state, &screen, &model, &effort, messages).await
}

/// One answered turn, provider choice, budget and all.
///
/// Split out of [`ask_claude`] so the shared chat the companion server runs
/// goes through exactly the same path: the same context, the same provider
/// resolution, the same cap, and the same spend written to the same key. A
/// second implementation would have been a second set of rules about money.
pub(crate) async fn answer(
    state: &AppState,
    screen: &str,
    model: &str,
    effort: &str,
    messages: Vec<ChatMessage>,
) -> Result<ChatReply, String> {
    // The whole thread is forwarded to Anthropic or written to the CLI's
    // stdin, so it is bounded here rather than discovered as a bill or a
    // rejected request.
    check_thread_size(&messages)?;
    check_screen(screen)?;
    let cli = chat_cli::find_cli();
    let config = state.config.lock().await.clone();
    let api_key = state.engine.api_key(&config).await;
    let provider = resolve_provider(&config, api_key.is_some(), cli.is_some());
    // The cap is enforced here rather than in the panel, which cannot be the
    // authority on money: it knows only the conversation in front of it, and
    // it prices turns it did not pay for.
    let key = spend_key(screen, config.active_league_id.as_deref());
    let spent = config.chat_spend_usd.get(&key).copied().unwrap_or(0.0);
    check_budget(spent, budget_of(&config), screen)?;
    // The cap above is read before the turn and written after it, so two
    // questions asked at once both saw the spend from before either of them.
    // The claim is held for the rest of this function and released by every
    // path out of it, `?` included.
    let _in_flight = chat_client::reserve(&key)?;

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

    let model = ChatModel::parse(model);
    let effort = Effort::parse(effort);
    let mut reply = if provider == PROVIDER_CLI {
        let cli = cli.ok_or("Claude Code CLI not found — install it or add an API key")?;
        chat_cli::ask(&cli, model, effort, &context, &messages).await?
    } else {
        let api_key = api_key.ok_or("no Anthropic API key set — add one in Settings")?;
        chat::ask(
            // Not the Sleeper client: its eight-second budget cut off every
            // answer that took longer than a board refresh.
            &chat_client::client(),
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
        chat::turn_cost_of(billed_model(model, &reply.model), &reply)
    };
    reply.provider = provider.to_string();
    let mut config = state.config.lock().await;
    let running = config.chat_spend_usd.entry(key).or_insert(0.0);
    *running += reply.cost_usd;
    reply.screen_spend_usd = *running;
    // A failure to write it down is not a reason to withhold the answer the
    // user already paid for; the next turn re-reads whatever did land.
    if let Err(e) = state.engine.save_config(&config) {
        crate::applog::warn(format!("could not record what Ask Claude spent: {e}"));
    }
    Ok(reply)
}

/// Suggested prompts for the current screen.
#[tauri::command]
pub fn chat_suggestions(screen: String) -> Vec<String> {
    chat_context::suggestions(&screen)
}

#[cfg(test)]
#[path = "commands_chat_tests.rs"]
mod tests;
