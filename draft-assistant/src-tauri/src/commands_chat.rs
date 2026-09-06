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

/// The pure rules: screens, the thread window, the cap and the spend key.
#[path = "commands_chat_budget.rs"]
mod budget;

use budget::{billed_model, charged_league, check_budget, check_screen, checked_budget, window};
pub use budget::{budget_of, spend_key, DEFAULT_BUDGET_USD};
#[cfg(test)]
use budget::{MAX_THREAD_BYTES, MAX_TURNS};

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

/// The ids a failure in this panel should be tied to: the screen that asked,
/// and the league whose board the question was about.
///
/// Without them a logged failure names the command and nothing else, and "ask
/// claude failed" read back a week later does not say which screen it was on
/// or which league was open. Commands with no screen of their own pass `""`,
/// which [`crate::applog::context`] drops rather than writing `screen=`.
async fn ids(state: &AppState, screen: &str) -> String {
    let league = {
        let loaded = state.loaded.lock().await;
        loaded
            .as_ref()
            .map(|l| l.league.league_id.clone())
            .unwrap_or_default()
    };
    crate::applog::context(&[("screen", screen), ("league", &league)])
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
    crate::applog::logged!(
        "set_api_key",
        ids(&state, "").await,
        set_api_key_inner(&state, key).await
    )
}

async fn set_api_key_inner(state: &AppState, key: String) -> Result<bool, String> {
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
    crate::applog::logged!(
        "set_chat_provider",
        ids(&state, "").await,
        set_chat_provider_inner(&state, provider).await
    )
}

async fn set_chat_provider_inner(
    state: &AppState,
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
    crate::applog::logged!(
        "chat_settings",
        ids(&state, "").await,
        chat_settings_inner(&state).await
    )
}

async fn chat_settings_inner(state: &AppState) -> Result<ChatSettings, String> {
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

/// Set the dollar cap a screen's chat runs under. Zero turns it off.
#[tauri::command]
pub async fn set_chat_budget(state: State<'_, AppState>, dollars: f64) -> Result<f64, String> {
    crate::applog::logged!(
        "set_chat_budget",
        ids(&state, "").await,
        set_chat_budget_inner(&state, dollars).await
    )
}

async fn set_chat_budget_inner(state: &AppState, dollars: f64) -> Result<f64, String> {
    let dollars = checked_budget(dollars)?;
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
    crate::applog::logged!(
        "ask_claude",
        ids(&state, &screen).await,
        answer(&state, &screen, &model, &effort, messages).await
    )
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
    // The screen is checked before anything is done with the thread: it keys
    // the spend, and a name that is not a screen must not get as far as
    // reading the conversation, let alone opening a tally under itself.
    check_screen(screen)?;
    // The whole thread is forwarded to Anthropic or written to the CLI's
    // stdin, so it is bounded here rather than discovered as a bill or a
    // rejected request.
    let messages = window(&messages)?.to_vec();
    let cli = chat_cli::find_cli();
    let config = state.config.lock().await.clone();
    let api_key = state.engine.api_key(&config).await;
    let provider = resolve_provider(&config, api_key.is_some(), cli.is_some());
    let loaded_league = {
        let loaded = state.loaded.lock().await;
        loaded.as_ref().map(|l| l.league.league_id.clone())
    };
    // The cap is enforced here rather than in the panel, which cannot be the
    // authority on money: it knows only the conversation in front of it, and
    // it prices turns it did not pay for.
    let key = spend_key(
        screen,
        charged_league(loaded_league.as_deref(), config.active_league_id.as_deref()),
    );
    let spent = config.chat_spend_usd.get(&key).copied().unwrap_or(0.0);
    check_budget(spent, budget_of(&config), screen)?;
    // The cap above is read before the turn and written after it, so two
    // questions asked at once both saw the spend from before either of them.
    // The claim travels with the model call and is released when it ends.
    let in_flight = chat_client::reserve(&key)?;

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
        chat_context::season_split(&view)
    } else {
        let loaded = state.loaded.lock().await;
        let loaded = loaded.as_ref().ok_or("no league loaded")?;
        let config = state.config.lock().await;
        chat_context::draft_split(&view_from(loaded, &config))
    };

    let model = ChatModel::parse(model);
    let effort = Effort::parse(effort);
    let books = Books {
        config: state.config.clone(),
        engine: state.engine.clone(),
        key,
        model,
        provider,
    };
    let call = async move {
        if provider == PROVIDER_CLI {
            let cli = cli.ok_or_else(|| {
                "Claude Code CLI not found — install it or add an API key".to_string()
            })?;
            chat_cli::ask(&cli, model, effort, &context.joined(), &messages)
                .await
                .map_err(chat::ChatError::from)
        } else {
            let api_key = api_key
                .ok_or_else(|| "no Anthropic API key set — add one in Settings".to_string())?;
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
            .await
        }
    };
    settle(books, in_flight, call).await
}

/// Where one turn's money is written down.
pub(crate) struct Books {
    pub(crate) config: std::sync::Arc<Mutex<AppConfig>>,
    pub(crate) engine: std::sync::Arc<crate::engine::Engine>,
    pub(crate) key: String,
    pub(crate) model: ChatModel,
    pub(crate) provider: &'static str,
}

impl Books {
    /// What a reply — or the billed part of a failed one — cost.
    ///
    /// The CLI route is paid for by a subscription, not by the token:
    /// charging it list rates would stop the panel over money nobody spent.
    fn cost_of(&self, reply: &ChatReply) -> f64 {
        if self.provider == PROVIDER_CLI {
            0.0
        } else {
            chat::turn_cost_of(billed_model(self.model, &reply.model), reply)
        }
    }

    /// Add `cost` to the running spend and return the new total.
    async fn record(&self, cost: f64) -> f64 {
        let mut config = self.config.lock().await;
        let running = config.chat_spend_usd.entry(self.key.clone()).or_insert(0.0);
        *running += cost;
        let running = *running;
        // A failure to write it down is not a reason to withhold the answer
        // the user already paid for; the next turn re-reads whatever did land.
        if let Err(e) = self.engine.save_config(&config) {
            crate::applog::warn(format!("could not record what Ask Claude spent: {e}"));
        }
        running
    }
}

/// Run the model call to its end and write down what it cost, whatever
/// becomes of the caller.
///
/// The call runs on a task of its own, so a caller that stops waiting — the
/// shared thread's answer limit, or a webview that went away — does not
/// cancel it. That matters because cancelling the future does not cancel the
/// bill: the API charges from the moment it accepts the request, and a turn
/// that was aborted at the await used to be billed, discarded, and never
/// counted against the cap. Here the spend is recorded by the same task that
/// made the call, before anything is handed back, and a call that fails after
/// the API started answering records the usage that did arrive.
pub(crate) async fn settle<F>(
    books: Books,
    in_flight: chat_client::InFlight,
    call: F,
) -> Result<ChatReply, String>
where
    F: std::future::Future<Output = Result<ChatReply, chat::ChatError>> + Send + 'static,
{
    let task = tokio::spawn(async move {
        // Released when the call ends, not when the caller stops waiting.
        let _in_flight = in_flight;
        match call.await {
            Ok(mut reply) => {
                reply.cost_usd = books.cost_of(&reply);
                reply.provider = books.provider.to_string();
                reply.screen_spend_usd = books.record(reply.cost_usd).await;
                Ok(reply)
            }
            Err(error) => {
                if let Some(partial) = &error.partial {
                    books.record(books.cost_of(partial)).await;
                }
                Err(error.message)
            }
        }
    });
    task.await
        .map_err(|_| "The answer stopped unexpectedly".to_string())?
}

/// Suggested prompts for the current screen.
#[tauri::command]
pub fn chat_suggestions(screen: String) -> Vec<String> {
    chat_context::suggestions(&screen)
}

#[cfg(test)]
#[path = "commands_chat_tests.rs"]
mod tests;
