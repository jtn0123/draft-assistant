//! Tauri commands for the AI chat panel.

use crate::chat::{ChatMessage, ChatModel, ChatReply, Effort};
use crate::chat_client;
use crate::chat_context;
use crate::chat_copy;
use crate::engine::AppConfig;
use crate::state::{season_view_for_chat, view_from, AppState};
use tauri::State;
use tokio::sync::Mutex;

/// The pure rules: screens, thread windows, legacy inputs and cost tally keys.
#[path = "commands_chat_budget.rs"]
mod budget;

/// The books one turn is written into, and the task that writes them.
#[path = "commands_chat_settle.rs"]
mod settle;

/// The model call itself, on whichever route was resolved.
#[path = "commands_chat_route.rs"]
mod route;

/// The panel's window onto an answer that is still being written.
#[path = "commands_chat_progress.rs"]
mod progress;

use budget::{billed_model, charged_league, check_screen, checked_budget, window};
pub use budget::{budget_of, spend_key, DEFAULT_BUDGET_USD};
#[cfg(test)]
use budget::{MAX_THREAD_BYTES, MAX_TURNS};
use progress::progress_events;
use route::Route;
pub(crate) use settle::{settle, Books};

const PROVIDER_API: &str = "api";
const PROVIDER_CLI: &str = "claude_code";

#[derive(serde::Serialize)]
pub struct ChatSettings {
    has_key: bool,
    /// The stored key, masked, for confirmation in Settings.
    key_hint: Option<String>,
    /// Whether the Claude Code CLI was found on this machine.
    cli_available: bool,
    /// Whether the ChatGPT-authenticated Codex CLI can be found.
    codex_available: bool,
    /// "api" or "claude_code" — the one answers will go through.
    provider: &'static str,
    /// Where the key is kept: "keychain" or "file".
    key_store: &'static str,
    /// Legacy compatibility field: always zero; chat has no spending limit.
    budget_usd: f64,
    /// `screen.league_id` -> what that screen's chats about that league have
    /// cost so far as an API-equivalent estimate, all conversations together.
    /// Bare-screen keys are from an older scheme and are not read;
    /// see [`spend_key`].
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
pub(crate) fn resolve_provider(cli_available: bool) -> &'static str {
    // Claude Code wins whenever it is installed — whatever is stored, and
    // whatever key the app is holding. Answers come out of the subscription
    // the CLI is signed into, and nothing here quietly bills a key per token.
    // The API route survives only as the way a machine *without* the CLI can
    // answer at all; the panel does not offer it as a choice any more, and
    // `chat_provider` in the config is no longer read.
    if cli_available {
        PROVIDER_CLI
    } else {
        PROVIDER_API
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
/// config is re-read *after* the wait, edited on a copy, encoded, swapped in,
/// and only then written, with the lock released again for the write.
/// Nothing the pollers did while the Keychain was busy is rolled back, and a
/// write that fails puts the one field back so memory and disk agree.
async fn store_key_unlocked<F, Fut, S, W>(
    config: &Mutex<AppConfig>,
    store: F,
    save: S,
) -> Result<(), String>
where
    F: FnOnce() -> Fut,
    Fut: std::future::Future<Output = Result<Option<String>, String>>,
    S: FnOnce(&AppConfig) -> Result<W, String>,
    W: Finish,
{
    let in_file = store().await?;
    let mut guard = config.lock().await;
    let mut next = guard.clone();
    let previous = std::mem::replace(&mut next.anthropic_api_key, in_file);
    let write = save(&next)?;
    *guard = next;
    drop(guard);
    if let Err(why) = write.finish().await {
        config.lock().await.anthropic_api_key = previous;
        return Err(why);
    }
    Ok(())
}

/// What `store_key_unlocked`'s save still has to do once the lock is
/// released: nothing, for a save that finished under it (the tests), or the
/// write itself, for one that was only prepared there.
trait Finish {
    fn finish(self) -> impl std::future::Future<Output = Result<(), String>>;
}

impl Finish for () {
    async fn finish(self) -> Result<(), String> {
        Ok(())
    }
}

/// A config write prepared under the lock and run after it.
struct Deferred<Fut>(Fut);

impl<Fut: std::future::Future<Output = Result<(), String>>> Finish for Deferred<Fut> {
    async fn finish(self) -> Result<(), String> {
        self.0.await
    }
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
        |config| {
            state
                .engine
                .prepare_config_save(config)
                .map(|pending| Deferred(pending.write()))
        },
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
    let mut guard = state.config.lock().await;
    guard.chat_provider = Some(chosen.to_string());
    let pending = state.engine.prepare_config_save(&guard)?;
    // Dropped so the write below does not hold the lock.
    drop(guard);
    pending.write().await?;
    // The stored choice is remembered but no longer decides anything: the
    // route is the CLI whenever there is one. What comes back is what will
    // actually answer, which is what the panel reports.
    Ok(resolve_provider(
        state.engine.chat_cli(ChatModel::Opus5).is_some(),
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
    let cli_available = state.engine.chat_cli(ChatModel::Opus5).is_some();
    let mut efforts = std::collections::HashMap::new();
    efforts.insert("Opus 5", chat_copy::effort_levels(ChatModel::Opus5));
    efforts.insert("Fable 5.1", chat_copy::effort_levels(ChatModel::Fable5));
    efforts.insert("GPT-6 Astra", chat_copy::effort_levels(ChatModel::Astra));
    efforts.insert("GPT-5.6 Sol", chat_copy::effort_levels(ChatModel::Sol));
    let mut notes = std::collections::HashMap::new();
    for label in ["Off", "Low", "Medium", "High", "xhigh", "Max"] {
        let (title, foot) = chat_copy::effort_note(Effort::parse(label));
        notes.insert(label, [title, foot]);
    }
    Ok(ChatSettings {
        has_key: key.is_some(),
        key_hint: key.map(chat_copy::mask_key),
        cli_available,
        codex_available: state.engine.chat_cli(ChatModel::Sol).is_some(),
        provider: resolve_provider(cli_available),
        key_store: if state.engine.secret_store().is_some() {
            "keychain"
        } else {
            "file"
        },
        budget_usd: budget_of(&config),
        spend_usd: config.chat_spend_usd.clone(),
        models: vec!["Opus 5", "Fable 5.1", "GPT-6 Astra", "GPT-5.6 Sol"],
        efforts,
        notes,
    })
}

/// Compatibility for old clients: validates the input, stores and returns zero.
/// Positive inputs cannot restore a spending limit.
#[tauri::command]
pub async fn set_chat_budget(state: State<'_, AppState>, dollars: f64) -> Result<f64, String> {
    crate::applog::logged!(
        "set_chat_budget",
        ids(&state, "").await,
        set_chat_budget_inner(&state, dollars).await
    )
}

async fn set_chat_budget_inner(state: &AppState, dollars: f64) -> Result<f64, String> {
    checked_budget(dollars)?;
    let dollars = 0.0;
    let pending = {
        let mut config = state.config.lock().await;
        config.chat_budget_usd = Some(dollars);
        state.engine.prepare_config_save(&config)?
    };
    pending.write().await?;
    Ok(dollars)
}

/// Ask Claude about the board or the week. `screen` selects which view is
/// summarised into the system prompt.
#[tauri::command]
pub async fn ask_claude<R: tauri::Runtime>(
    app: tauri::AppHandle<R>,
    state: State<'_, AppState>,
    screen: String,
    model: String,
    effort: String,
    messages: Vec<ChatMessage>,
) -> Result<ChatReply, String> {
    let watcher = progress_events(app, &screen);
    crate::applog::logged!(
        "ask_claude",
        ids(&state, &screen).await,
        answer(&state, &screen, &model, &effort, messages, Some(watcher)).await
    )
}

/// The key one screen's questions are claimed and charged under right now:
/// the screen, and the league whose board is open.
async fn current_key(state: &AppState, screen: &str) -> String {
    let loaded_league = {
        let loaded = state.loaded.lock().await;
        loaded.as_ref().map(|l| l.league.league_id.clone())
    };
    let active = state.config.lock().await.active_league_id.clone();
    spend_key(
        screen,
        charged_league(loaded_league.as_deref(), active.as_deref()),
    )
}

/// Claim this screen's one in-flight slot before anything else is done with
/// a question, or say why not.
///
/// The shared thread used to post a phone's question first and claim second,
/// so a question asked while the desktop panel was mid-answer was accepted
/// with a 202 and then failed in the thread as "already being answered". The
/// claim now comes first on both paths, and a refusal is a refusal up front.
pub(crate) async fn claim(state: &AppState, screen: &str) -> Result<chat_client::InFlight, String> {
    check_screen(screen)?;
    state.chat_claims.reserve(&current_key(state, screen).await)
}

/// Stop the answer this screen is waiting on, if there is one. True when a
/// question was in flight to stop; its reply comes back through the call that
/// asked it, marked cut short, with the text that had arrived.
#[tauri::command]
pub async fn cancel_claude(state: State<'_, AppState>, screen: String) -> Result<bool, String> {
    crate::applog::logged!(
        "cancel_claude",
        ids(&state, &screen).await,
        cancel_claude_inner(&state, &screen).await
    )
}

async fn cancel_claude_inner(state: &AppState, screen: &str) -> Result<bool, String> {
    check_screen(screen)?;
    Ok(state.chat_claims.cancel(&current_key(state, screen).await))
}

/// One answered turn, including provider choice and estimated cost.
///
/// Split out of [`ask_claude`] so the shared chat the companion server runs
/// goes through exactly the same path: the same context, the same provider
/// resolution, and the same estimated cost written to the same key.
pub(crate) async fn answer(
    state: &AppState,
    screen: &str,
    model: &str,
    effort: &str,
    messages: Vec<ChatMessage>,
    progress: Option<crate::chat::OnProgress>,
) -> Result<ChatReply, String> {
    // The screen is checked before anything is done with the thread: it keys
    // the spend, and a name that is not a screen must not get as far as
    // reading the conversation, let alone opening a tally under itself.
    let held = claim(state, screen).await?;
    answer_holding(state, screen, model, effort, messages, held, progress).await
}

/// [`answer`] with the in-flight claim already made by the caller: the shared
/// thread claims before it posts the question, and hands the claim on here.
pub(crate) async fn answer_holding(
    state: &AppState,
    screen: &str,
    model: &str,
    effort: &str,
    messages: Vec<ChatMessage>,
    held: chat_client::InFlight,
    progress: Option<crate::chat::OnProgress>,
) -> Result<ChatReply, String> {
    check_screen(screen)?;
    // The whole thread is forwarded to Anthropic or written to the CLI's
    // stdin, so it is bounded here rather than discovered as a bill or a
    // rejected request.
    let messages = window(&messages)?.to_vec();
    let model = ChatModel::parse(model);
    let cli = state.engine.chat_cli(model);
    let config = state.config.lock().await.clone();
    let api_key = if model.is_openai() {
        None
    } else {
        state.engine.api_key(&config).await
    };
    let provider = if model.is_openai() {
        "codex"
    } else {
        resolve_provider(cli.is_some())
    };
    // The backend records estimated cost under the current screen and league,
    // including answers requested from a paired device.
    let key = current_key(state, screen).await;
    // The claim travels with the model call and is released when it ends. One
    // made under a key that has since changed (the league switched between
    // the claim and the call) is let go for one under the key being charged.
    let in_flight = if held.key() == key {
        held
    } else {
        drop(held);
        state.chat_claims.reserve(&key)?
    };
    // Estimates are recorded after the answer; old saved caps are ignored.

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

    let effort = Effort::parse(effort);
    let books = Books {
        config: state.config.clone(),
        engine: state.engine.clone(),
        key,
        model,
        provider,
    };
    let cancel = in_flight.signal();
    let route = Route {
        provider,
        cli,
        api_key,
        model,
        effort,
        context,
        messages,
    };
    settle(books, in_flight, route.call(cancel, progress)).await
}

/// Suggested prompts for the current screen.
#[tauri::command]
pub fn chat_suggestions(screen: String) -> Vec<String> {
    chat_context::suggestions(&screen)
}

#[cfg(test)]
#[path = "commands_chat_tests.rs"]
mod tests;

#[cfg(test)]
#[path = "commands_chat_cancel_tests.rs"]
mod cancel_tests;
