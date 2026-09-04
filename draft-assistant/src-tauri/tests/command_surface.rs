//! The one place `lib.rs` is exercised rather than merely compiled.
//!
//! Every other test in this crate calls a function directly. Nothing stands
//! the Tauri app up and pushes a message through it, so the wiring between a
//! `#[tauri::command]` and the name the frontend types into `invoke()` was
//! only ever checked by running the app by hand. A command added to
//! `commands_draft.rs` and forgotten in `generate_handler!` compiles, ships,
//! and fails at the user.
//!
//! This builds the real command surface on Tauri's mock runtime and sends
//! each command a real IPC request, asserting the dispatcher recognises it.
//! Most then fail on "no league loaded" or a missing argument, which is the
//! point: reaching a command's own error means the routing worked.
//!
//! The list below is duplicated from `lib.rs` on purpose, and
//! `handler_list_matches_lib_rs` fails if the two ever drift -- so the
//! duplicate is a second opinion rather than a second source of truth.
//!
//! The two polling commands take `AppHandle<R>` rather than a bare
//! `tauri::AppHandle` precisely so they can be registered here too: every
//! command in the list makes the round trip.

use std::collections::BTreeSet;
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicBool, AtomicU64};
use std::sync::Arc;

use draft_assistant_lib::commands_chat as chat;
use draft_assistant_lib::commands_draft as draft;
use draft_assistant_lib::commands_season as season;
use draft_assistant_lib::commands_second_opinion as second_opinion;
use draft_assistant_lib::commands_yahoo as yahoo;
use draft_assistant_lib::engine::Engine;
use draft_assistant_lib::leagues;
use draft_assistant_lib::state::{AppState, YahooState};
use tauri::ipc::{CallbackFn, InvokeBody};
use tauri::test::{get_ipc_response, mock_builder, mock_context, noop_assets, INVOKE_KEY};
use tauri::webview::InvokeRequest;
use tauri::{Manager, WebviewWindowBuilder};
use tokio::sync::Mutex;

fn src_dir() -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR")).join("src")
}

/// The command names `lib.rs` actually hands to `generate_handler!`.
fn registered_in_lib_rs() -> BTreeSet<String> {
    let source = std::fs::read_to_string(src_dir().join("lib.rs")).expect("read lib.rs");
    let start = source
        .find("tauri::generate_handler![")
        .expect("lib.rs has a generate_handler! list");
    let body = &source[start + "tauri::generate_handler![".len()..];
    let end = body.find(']').expect("generate_handler! list is closed");
    body[..end]
        .split(',')
        .map(str::trim)
        .filter(|name| !name.is_empty())
        .map(str::to_string)
        .collect()
}

/// Every `#[tauri::command]` declared anywhere in the crate.
fn declared_in_crate() -> BTreeSet<String> {
    fn walk(dir: &Path, found: &mut BTreeSet<String>) {
        for entry in std::fs::read_dir(dir).expect("read source dir") {
            let path = entry.expect("read dir entry").path();
            if path.is_dir() {
                walk(&path, found);
            } else if path.extension().is_some_and(|ext| ext == "rs") {
                collect(
                    &std::fs::read_to_string(&path).expect("read source file"),
                    found,
                );
            }
        }
    }

    fn collect(source: &str, found: &mut BTreeSet<String>) {
        let mut lines = source.lines();
        while let Some(line) = lines.next() {
            if line.trim() != "#[tauri::command]" {
                continue;
            }
            // Skip any further attributes or doc comments before the signature.
            let signature = lines
                .by_ref()
                .find(|line| line.contains("fn "))
                .expect("a #[tauri::command] is followed by a function");
            let name = signature
                .split("fn ")
                .nth(1)
                .expect("signature has a name")
                .split(['(', '<'])
                .next()
                .expect("name is delimited");
            found.insert(name.trim().to_string());
        }
    }

    let mut found = BTreeSet::new();
    walk(&src_dir(), &mut found);
    found
}

/// Commands that open native UI when invoked. They are registered on the
/// mock runtime like everything else, so the wiring is checked, but calling
/// one here would either pop a real file picker or panic for want of the
/// dialog plugin's state, which the mock app has no window to host --- or, in
/// `yahoo_begin_connect`'s case, hand a URL to the machine's browser.
const OPENS_NATIVE_UI: [&str; 2] = ["import_second_opinion", "yahoo_begin_connect"];

/// The full command list, as `lib.rs` should have it.
fn handler_list() -> BTreeSet<String> {
    [
        "add_league",
        "set_my_username",
        "get_config",
        "sleeper_leagues",
        "remove_league",
        "get_state",
        "refresh_picks",
        "refresh_data",
        "record_manual_pick",
        "undo_manual_pick",
        "export_state",
        "start_polling",
        "stop_polling",
        "load_season",
        "get_season",
        "refresh_season",
        "start_season_polling",
        "stop_season_polling",
        "headshot",
        "avatar",
        "set_api_key",
        "set_chat_provider",
        "set_chat_budget",
        "chat_settings",
        "ask_claude",
        "chat_suggestions",
        "import_second_opinion",
        "yahoo_status",
        "yahoo_save_credentials",
        "yahoo_begin_connect",
        "yahoo_finish_connect",
        "yahoo_disconnect",
        "yahoo_leagues",
    ]
    .into_iter()
    .map(str::to_string)
    .collect()
}

/// A command that exists but never reaches `generate_handler!` is invisible to
/// the frontend, and nothing else in this crate would notice.
#[test]
fn every_command_is_registered() {
    assert_eq!(
        declared_in_crate(),
        registered_in_lib_rs(),
        "a #[tauri::command] is missing from (or stale in) lib.rs's generate_handler! list",
    );
}

/// Keeps this file's copy of the list honest.
#[test]
fn handler_list_matches_lib_rs() {
    assert_eq!(
        handler_list(),
        registered_in_lib_rs(),
        "tests/command_surface.rs and lib.rs disagree about the command list",
    );
}

/// Boots the app on the mock runtime with the same state `lib.rs`'s `setup`
/// installs, then asks every command to answer over the IPC.
#[test]
fn every_command_answers_over_the_ipc() {
    let data_dir = std::env::temp_dir().join(format!(
        "draft-assistant-command-surface-{}",
        std::process::id()
    ));
    std::fs::create_dir_all(&data_dir).expect("create test data dir");

    let engine = Engine::new(data_dir.clone());
    let config = engine.load_config();
    let app = mock_builder()
        .invoke_handler(tauri::generate_handler![
            draft::add_league,
            draft::set_my_username,
            draft::get_config,
            leagues::sleeper_leagues,
            leagues::remove_league,
            draft::get_state,
            draft::refresh_picks,
            draft::refresh_data,
            draft::record_manual_pick,
            draft::undo_manual_pick,
            draft::export_state,
            draft::start_polling,
            draft::stop_polling,
            season::load_season,
            season::get_season,
            season::refresh_season,
            season::start_season_polling,
            season::stop_season_polling,
            season::headshot,
            season::avatar,
            chat::set_api_key,
            chat::set_chat_provider,
            chat::set_chat_budget,
            chat::chat_settings,
            chat::ask_claude,
            chat::chat_suggestions,
            second_opinion::import_second_opinion,
            yahoo::yahoo_status,
            yahoo::yahoo_save_credentials,
            yahoo::yahoo_begin_connect,
            yahoo::yahoo_finish_connect,
            yahoo::yahoo_disconnect,
            yahoo::yahoo_leagues,
        ])
        .build(mock_context(noop_assets()))
        .expect("the app builds on the mock runtime");

    app.manage(AppState {
        engine: Arc::new(engine),
        loaded: Arc::new(Mutex::new(None)),
        season: Arc::new(Mutex::new(None)),
        config: Arc::new(Mutex::new(config)),
        polling: Arc::new(AtomicBool::new(false)),
        poll_generation: Arc::new(AtomicU64::new(0)),
        season_polling: Arc::new(AtomicBool::new(false)),
        season_generation: Arc::new(AtomicU64::new(0)),
        last_season_view: Arc::new(Mutex::new(None)),
        yahoo: Arc::new(YahooState::default()),
    });

    let webview = WebviewWindowBuilder::new(&app, "main", Default::default())
        .build()
        .expect("the main webview builds");

    for command in handler_list() {
        if OPENS_NATIVE_UI.contains(&command.as_str()) {
            continue;
        }
        let response = get_ipc_response(
            &webview,
            InvokeRequest {
                cmd: command.clone(),
                callback: CallbackFn(0),
                error: CallbackFn(1),
                url: "tauri://localhost".parse().expect("valid url"),
                body: InvokeBody::default(),
                headers: Default::default(),
                invoke_key: INVOKE_KEY.to_string(),
            },
        );
        // Called with no arguments and no league loaded, most of these fail --
        // on a missing argument or on "no league loaded". Either answer proves
        // the dispatcher routed the name to a real command. Only the
        // dispatcher's own "not found" means the wiring is broken.
        if let Err(error) = response {
            let text = error.to_string();
            assert!(
                !text.contains("not found") && !text.contains("not allowed"),
                "invoke(\"{command}\") was not routed to a command: {text}",
            );
        }
    }

    // Two of those commands started a poll loop against the real Sleeper host
    // — the stub is not installed here — and nothing stopped them. Left
    // running they outlive the test, retrying on the network every few
    // seconds for as long as the binary is alive.
    for command in ["stop_polling", "stop_season_polling"] {
        get_ipc_response(
            &webview,
            InvokeRequest {
                cmd: command.to_string(),
                callback: CallbackFn(0),
                error: CallbackFn(1),
                url: "tauri://localhost".parse().expect("valid url"),
                body: InvokeBody::default(),
                headers: Default::default(),
                invoke_key: INVOKE_KEY.to_string(),
            },
        )
        .unwrap_or_else(|error| panic!("{command} failed: {error}"));
    }

    std::fs::remove_dir_all(&data_dir).ok();
}
