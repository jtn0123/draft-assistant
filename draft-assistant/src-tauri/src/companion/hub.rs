//! Everything the companion server keeps between requests: the pairing code,
//! the paired devices, the event fan-out, and the two rate limits.
//!
//! The hub is deliberately separate from the running HTTP server. It is
//! created once at startup and managed by Tauri, so the poll loops can publish
//! into it without caring whether anyone is listening; turning the server on
//! and off only swaps what is in [`HubInner::running`].

use super::rand;
use serde::Serialize;
use std::sync::{Arc, Mutex};
use std::time::{Duration, SystemTime, UNIX_EPOCH};
use tokio::sync::broadcast;

/// How anything in here reaches the host's own webview. A closure rather than
/// an `AppHandle` so nothing below this line is generic over the Tauri
/// runtime, and so the tests can stand a hub up with no Tauri at all.
pub type Emit = Arc<dyn Fn(&str, serde_json::Value) + Send + Sync>;

/// A paired phone or follower desktop, as the contract describes it.
#[derive(Debug, Clone, Serialize)]
pub struct Device {
    pub device_id: String,
    pub name: String,
    /// "phone" or "desktop".
    pub kind: String,
    pub paired_at_ms: u64,
    pub last_seen_ms: u64,
    pub connected: bool,
}

/// One paired device plus the secret nobody outside this module sees.
#[derive(Debug, Clone)]
struct Paired {
    token: String,
    device: Device,
    /// Open WebSockets for this device. `connected` is this being non-zero.
    sockets: u32,
    /// When this device posted its recent chat questions, for the per-minute cap.
    posts: Vec<u64>,
}

/// Five wrong codes inside this window locks pairing out.
const PAIR_WINDOW_MS: u64 = 60_000;
const PAIR_MAX_FAILURES: usize = 5;
const PAIR_LOCKOUT_MS: u64 = 60_000;
/// Questions one device may post per minute.
const CHAT_MAX_PER_MINUTE: usize = 10;
const CHAT_WINDOW_MS: u64 = 60_000;
/// How many events a slow client may fall behind before it is dropped.
const EVENT_BACKLOG: usize = 64;

pub fn now_ms() -> u64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .unwrap_or(Duration::ZERO)
        .as_millis() as u64
}

struct HubInner {
    code: String,
    devices: Vec<Paired>,
    /// Times of the recent wrong codes, and when a lockout ends.
    failures: Vec<u64>,
    locked_until_ms: u64,
    /// The port the server is listening on, when it is.
    port: Option<u16>,
    host_name: String,
    /// Set once at startup. Absent in the tests, which have no webview.
    emit: Option<Emit>,
}

/// What both the phone and the desktop read the companion's world through.
pub struct CompanionHub {
    inner: Mutex<HubInner>,
    events: broadcast::Sender<String>,
}

/// The outcome of an attempt to pair.
pub enum PairOutcome {
    Ok {
        token: String,
        device_id: String,
        host_name: String,
    },
    WrongCode,
    LockedOut,
}

impl CompanionHub {
    /// A hub with a fresh code and nothing paired. Fails only if the machine's
    /// random source cannot be read, which is not a state to carry on from: a
    /// guessable pairing code is the one thing this must never have.
    pub fn new(host_name: String) -> Result<Self, String> {
        let (events, _) = broadcast::channel(EVENT_BACKLOG);
        Ok(Self {
            inner: Mutex::new(HubInner {
                code: rand::pairing_code()?,
                devices: Vec::new(),
                failures: Vec::new(),
                locked_until_ms: 0,
                port: None,
                host_name,
                emit: None,
            }),
            events,
        })
    }

    fn lock(&self) -> std::sync::MutexGuard<'_, HubInner> {
        // A panic while the hub was locked would otherwise take the whole
        // companion server down with it for the rest of the session. Nothing
        // under this lock can leave a half-updated invariant behind, so the
        // poisoned guard is the right thing to carry on with.
        self.inner.lock().unwrap_or_else(|e| e.into_inner())
    }

    pub fn code(&self) -> String {
        self.lock().code.clone()
    }

    pub fn host_name(&self) -> String {
        self.lock().host_name.clone()
    }

    pub fn set_host_name(&self, name: String) {
        self.lock().host_name = name;
    }

    /// Give the hub its way back to the host's own window.
    pub fn set_emit(&self, emit: Emit) {
        self.lock().emit = Some(emit);
    }

    /// Send one `{type, payload}` to the host's webview, if there is one.
    fn to_webview(&self, kind: &str, payload: serde_json::Value) {
        let emit = self.lock().emit.clone();
        if let Some(emit) = emit {
            emit(kind, payload);
        }
    }

    pub fn port(&self) -> Option<u16> {
        self.lock().port
    }

    pub fn set_port(&self, port: Option<u16>) {
        self.lock().port = port;
    }

    pub fn is_running(&self) -> bool {
        self.lock().port.is_some()
    }

    /// A new code, and every device thrown off. Returns the new code.
    pub fn revoke(&self) -> Result<String, String> {
        let code = rand::pairing_code()?;
        {
            let mut inner = self.lock();
            inner.code = code.clone();
            inner.devices.clear();
            inner.failures.clear();
            inner.locked_until_ms = 0;
        }
        // The sockets themselves are closed by the WebSocket task, which
        // notices its device is gone as soon as it wakes for this frame.
        self.publish_json("revoked", serde_json::json!({}));
        self.publish_devices();
        Ok(code)
    }

    /// Try to pair. The code is compared in constant time and five wrong ones
    /// inside a minute stop the sixth from being tried at all.
    pub fn pair(&self, code: &str, name: &str, kind: &str) -> Result<PairOutcome, String> {
        let now = now_ms();
        let token = rand::token()?;
        let device_id = rand::device_id()?;
        let outcome = {
            let mut inner = self.lock();
            if now < inner.locked_until_ms {
                return Ok(PairOutcome::LockedOut);
            }
            inner
                .failures
                .retain(|at| now.saturating_sub(*at) < PAIR_WINDOW_MS);
            if !rand::secrets_match(code, &inner.code) {
                inner.failures.push(now);
                if inner.failures.len() >= PAIR_MAX_FAILURES {
                    inner.locked_until_ms = now + PAIR_LOCKOUT_MS;
                    inner.failures.clear();
                }
                return Ok(PairOutcome::WrongCode);
            }
            inner.failures.clear();
            let kind = if kind == "desktop" {
                "desktop"
            } else {
                "phone"
            };
            // The same phone pairing again — a reinstalled page, a new code
            // after a revoke — replaces its old entry rather than sitting
            // beside it as a ghost; the old token dies with it.
            let name = display_name(name);
            inner
                .devices
                .retain(|d| !(d.device.name == name && d.device.kind == kind));
            inner.devices.push(Paired {
                token: token.clone(),
                device: Device {
                    device_id: device_id.clone(),
                    name,
                    kind: kind.to_string(),
                    paired_at_ms: now,
                    last_seen_ms: now,
                    connected: false,
                },
                sockets: 0,
                posts: Vec::new(),
            });
            PairOutcome::Ok {
                token,
                device_id,
                host_name: inner.host_name.clone(),
            }
        };
        self.publish_devices();
        Ok(outcome)
    }

    /// The device a bearer token belongs to, with its `last_seen` moved up.
    pub fn device_for(&self, token: &str) -> Option<Device> {
        let mut inner = self.lock();
        let now = now_ms();
        let found = inner
            .devices
            .iter_mut()
            .find(|d| rand::secrets_match(token, &d.token))?;
        found.device.last_seen_ms = now;
        Some(found.device.clone())
    }

    pub fn devices(&self) -> Vec<Device> {
        self.lock()
            .devices
            .iter()
            .map(|d| d.device.clone())
            .collect()
    }

    /// Note that a device opened or closed a WebSocket.
    pub fn socket_changed(&self, device_id: &str, opened: bool) {
        {
            let mut inner = self.lock();
            let Some(found) = inner
                .devices
                .iter_mut()
                .find(|d| d.device.device_id == device_id)
            else {
                return;
            };
            found.sockets = if opened {
                found.sockets + 1
            } else {
                found.sockets.saturating_sub(1)
            };
            found.device.connected = found.sockets > 0;
            found.device.last_seen_ms = now_ms();
        }
        self.publish_devices();
    }

    /// Whether this device may post another question right now.
    pub fn allow_chat_post(&self, device_id: &str) -> bool {
        let now = now_ms();
        let mut inner = self.lock();
        let Some(found) = inner
            .devices
            .iter_mut()
            .find(|d| d.device.device_id == device_id)
        else {
            return false;
        };
        found
            .posts
            .retain(|at| now.saturating_sub(*at) < CHAT_WINDOW_MS);
        if found.posts.len() >= CHAT_MAX_PER_MINUTE {
            return false;
        }
        found.posts.push(now);
        true
    }

    /// Whether the device behind this id is still paired. The WebSocket task
    /// asks after every frame so a revoke drops it.
    pub fn still_paired(&self, device_id: &str) -> bool {
        self.lock()
            .devices
            .iter()
            .any(|d| d.device.device_id == device_id)
    }

    /// A receiver for the server-sent event stream.
    pub fn subscribe(&self) -> broadcast::Receiver<String> {
        self.events.subscribe()
    }

    /// Fan one `{type, payload}` frame out to every open socket. Nothing is
    /// sent when nobody is listening, and a full channel is not an error —
    /// the events are a live feed, not a queue anyone replays.
    pub fn publish_json(&self, kind: &str, payload: serde_json::Value) {
        let frame = serde_json::json!({ "type": kind, "payload": payload });
        let _ = self.events.send(frame.to_string());
    }

    /// The same for anything serialisable. A value that will not serialise is
    /// dropped with a note rather than taking a poll tick down.
    pub fn publish<T: Serialize>(&self, kind: &str, payload: &T) {
        match serde_json::to_value(payload) {
            Ok(value) => self.publish_json(kind, value),
            Err(e) => crate::applog::warn(format!(
                "companion: could not send a '{kind}' update to the paired devices: {e}"
            )),
        }
    }

    /// The device list, to the paired devices and to the host's own settings
    /// screen. The webview event is named separately in the contract because
    /// the desktop already has a `devices` of its own meaning nothing like it.
    pub fn publish_devices(&self) {
        let devices = self.devices();
        self.publish("devices", &devices);
        match serde_json::to_value(&devices) {
            Ok(value) => self.to_webview("companion-devices", value),
            Err(e) => crate::applog::warn(format!("companion: could not list the devices: {e}")),
        }
    }
}

/// A device name that is worth showing: trimmed, bounded, never empty.
fn display_name(raw: &str) -> String {
    let trimmed = raw.trim();
    if trimmed.is_empty() {
        return "A device".to_string();
    }
    trimmed.chars().take(60).collect()
}

#[cfg(test)]
#[path = "hub_tests.rs"]
mod tests;
