//! A companion server on a real socket, with the fixture league loaded.
//!
//! No Tauri: the server takes an `AppState` and a closure back to the webview,
//! and both of those are ordinary values. What these tests drive is the same
//! server the desktop starts — a real listener, real HTTP, a real WebSocket.

#[path = "../common/mod.rs"]
pub mod common;

use draft_assistant_lib::companion::CompanionServer;
use draft_assistant_lib::engine::Engine;
use draft_assistant_lib::state::{AppState, YahooState};
use serde_json::Value;
use std::sync::atomic::{AtomicBool, AtomicU64};
use std::sync::{Arc, Mutex};
use tokio::sync::Mutex as AsyncMutex;

/// A running host, and everything the tests need to poke at it.
pub struct Host {
    pub base: String,
    pub companion: Arc<CompanionServer>,
    pub state: Arc<AppState>,
    pub data_dir: std::path::PathBuf,
    /// Every `(type, payload)` the host's own webview was sent.
    pub emitted: Arc<Mutex<Vec<(String, Value)>>>,
    pub http: reqwest::Client,
}

pub fn scratch_dir(label: &str) -> std::path::PathBuf {
    let unique = format!(
        "draft-assistant-companion-{label}-{}-{}",
        std::process::id(),
        std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .expect("clock")
            .as_nanos()
    );
    let dir = std::env::temp_dir().join(unique);
    std::fs::create_dir_all(&dir).expect("the scratch directory is creatable");
    dir
}

/// The app state the fixture league produces, with nothing polling.
pub fn fixture_state(data_dir: &std::path::Path) -> AppState {
    let (loaded, season, config) = common::fixture();
    AppState {
        engine: Arc::new(Engine::new(data_dir.to_path_buf())),
        loaded: Arc::new(AsyncMutex::new(Some(loaded))),
        season: Arc::new(AsyncMutex::new(Some(season))),
        config: Arc::new(AsyncMutex::new(config)),
        polling: Arc::new(AtomicBool::new(false)),
        poll_generation: Arc::new(AtomicU64::new(0)),
        season_polling: Arc::new(AtomicBool::new(false)),
        season_generation: Arc::new(AtomicU64::new(0)),
        last_season_view: Arc::new(AsyncMutex::new(None)),
        yahoo: Arc::new(YahooState::sandboxed(Default::default())),
        chat_claims: Arc::new(Default::default()),
    }
}

/// A companion server listening on an ephemeral port, with the fixture loaded.
pub async fn host(label: &str) -> Host {
    let data_dir = scratch_dir(label);
    let state = Arc::new(fixture_state(&data_dir));
    host_over(data_dir, state).await
}

/// The same, over state and a data directory the caller already has. This is
/// what a restart looks like: the threads on disk outlive the server.
pub async fn host_over(data_dir: std::path::PathBuf, state: Arc<AppState>) -> Host {
    host_over_tls(data_dir, state, None).await
}

/// The same, serving HTTPS from the given certificate beside the plain
/// listener. `None` is the sandboxed default: no certificate, no `tailscale`.
pub async fn host_over_tls(
    data_dir: std::path::PathBuf,
    state: Arc<AppState>,
    tls: Option<draft_assistant_lib::companion::tls::TlsSource>,
) -> Host {
    let companion = Arc::new(
        CompanionServer::sandboxed("Justin's Mac".to_string(), data_dir.clone())
            .expect("the companion builds"),
    );
    if let Some(tls) = tls {
        companion.set_tls(tls);
    }
    let emitted: Arc<Mutex<Vec<(String, Value)>>> = Arc::new(Mutex::new(Vec::new()));
    let sink = emitted.clone();
    companion.attach(
        state.clone(),
        Arc::new(move |kind: &str, payload: Value| {
            sink.lock()
                .unwrap_or_else(|e| e.into_inner())
                .push((kind.to_string(), payload));
        }),
    );
    // Port 0: the kernel picks a free one, so tests never collide with each
    // other or with a companion the developer happens to be running.
    let port = companion.start(0).await.expect("the server starts");
    Host {
        base: format!("http://127.0.0.1:{port}"),
        companion,
        state,
        data_dir,
        emitted,
        http: reqwest::Client::new(),
    }
}

impl Host {
    /// Pair with the code on the host's screen, and keep the token.
    pub async fn pair_ok(&self, name: &str, kind: &str) -> Paired {
        self.pair_again(name, kind, None).await
    }

    /// The same, saying which device this already was — what a phone that has
    /// paired before sends, and the only thing that replaces an old entry.
    pub async fn pair_again(&self, name: &str, kind: &str, previous: Option<&str>) -> Paired {
        let code = self.companion.hub.code();
        let response = self
            .http
            .post(format!("{}/api/pair", self.base))
            .json(&serde_json::json!({
                "code": code,
                "device_name": name,
                "kind": kind,
                "device_id": previous,
            }))
            .send()
            .await
            .expect("the pair request goes through");
        assert_eq!(response.status(), 200, "pairing with the right code");
        let body: Value = response.json().await.expect("pairing answers JSON");
        Paired {
            token: body["token"].as_str().expect("a token").to_string(),
            device_id: body["device_id"].as_str().expect("an id").to_string(),
            host_name: body["host_name"].as_str().expect("a name").to_string(),
        }
    }

    /// A GET with a bearer token, as `(status, body)`.
    pub async fn get(&self, path: &str, token: &str) -> (u16, Value) {
        let response = self
            .http
            .get(format!("{}{path}", self.base))
            .bearer_auth(token)
            .send()
            .await
            .expect("the request goes through");
        let status = response.status().as_u16();
        let body = response.text().await.expect("a body");
        (
            status,
            serde_json::from_str(&body).unwrap_or(Value::String(body)),
        )
    }

    /// A POST with a bearer token, as `(status, body)`.
    pub async fn post(&self, path: &str, token: &str, body: Value) -> (u16, Value) {
        let response = self
            .http
            .post(format!("{}{path}", self.base))
            .bearer_auth(token)
            .json(&body)
            .send()
            .await
            .expect("the request goes through");
        let status = response.status().as_u16();
        let text = response.text().await.expect("a body");
        (
            status,
            serde_json::from_str(&text).unwrap_or(Value::String(text)),
        )
    }

    /// The `{type, payload}` frames the host sent its own webview.
    pub fn emitted_kinds(&self) -> Vec<String> {
        self.emitted
            .lock()
            .unwrap_or_else(|e| e.into_inner())
            .iter()
            .map(|(kind, _)| kind.clone())
            .collect()
    }
}

pub struct Paired {
    pub token: String,
    pub device_id: String,
    pub host_name: String,
}

/// How long a poll below may take before it is a failure rather than a slow
/// machine. Generous on purpose: a test that waits on a condition finishes
/// as soon as it holds, so the deadline only ever costs anything when the
/// condition is never going to.
pub const DEADLINE: std::time::Duration = std::time::Duration::from_secs(10);

/// Poll `holds` until it is true, or panic with `what` after [`DEADLINE`].
///
/// Every wait in these tests goes through here rather than a fixed sleep: a
/// sleep long enough for a loaded CI runner is a sleep too long for the
/// laptop, and one short enough for the laptop is a flake on the runner.
pub async fn wait_until(what: &str, mut holds: impl FnMut() -> bool) {
    let started = std::time::Instant::now();
    while !holds() {
        assert!(
            started.elapsed() < DEADLINE,
            "gave up after {DEADLINE:?} waiting until {what}"
        );
        tokio::time::sleep(std::time::Duration::from_millis(10)).await;
    }
}

/// Whether something is listening on 127.0.0.1 at `port` right now.
pub fn port_is_open(port: u16) -> bool {
    std::net::TcpStream::connect_timeout(
        &std::net::SocketAddr::from(([127, 0, 0, 1], port)),
        std::time::Duration::from_millis(200),
    )
    .is_ok()
}

/// Wait for a stopped listener to have actually let go of its port, which
/// is what has to be true before the same port can be bound again.
pub async fn wait_for_port_closed(port: u16) {
    wait_until(&format!("port {port} has closed"), || !port_is_open(port)).await;
}
