//! Everything the companion server keeps between requests: the pairing code,
//! the paired devices, the event fan-out, and the two rate limits.
//!
//! The hub is deliberately separate from the running HTTP server. It is
//! created once at startup and managed by Tauri, so the poll loops can publish
//! into it without caring whether anyone is listening; turning the server on
//! and off only swaps what is in [`HubInner::running`].

use super::names::{display_name, unique_name};
use super::pairing::{Lockout, Paired};
use super::rand;
use super::store::{self, StoredDevice, StoredHub};
use serde::Serialize;
use std::path::PathBuf;
use std::sync::{Arc, Mutex};
use std::time::{Duration, SystemTime, UNIX_EPOCH};
use tokio::sync::broadcast;

pub use super::pairing::{Device, PairAttempt, PairOutcome};

/// How anything in here reaches the host's own webview. A closure rather than
/// an `AppHandle` so nothing below this line is generic over the Tauri
/// runtime, and so the tests can stand a hub up with no Tauri at all.
pub type Emit = Arc<dyn Fn(&str, serde_json::Value) + Send + Sync>;

/// How long a code nobody has used stays on screen before it is replaced.
pub const CODE_MAX_AGE_MS: u64 = 10 * 60_000;
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
    /// When the code on screen was made, for the idle rotation.
    code_at_ms: u64,
    devices: Vec<Paired>,
    lockout: Lockout,
    /// The port the server is listening on, when it is.
    port: Option<u16>,
    /// The `http://host:port` origins this server is actually reachable at,
    /// filled in when it starts listening. The cross-origin check and the
    /// page's `connect-src` are both built from this rather than from "any
    /// private address", so a page on another machine's LAN address cannot
    /// name itself into the allow-list.
    origins: Vec<String>,
    host_name: String,
    /// Set once at startup. Absent in the tests, which have no webview.
    emit: Option<Emit>,
}

/// What both the phone and the desktop read the companion's world through.
pub struct CompanionHub {
    inner: Mutex<HubInner>,
    events: broadcast::Sender<String>,
    /// Tokens that have stopped being valid while a socket still held them.
    /// A socket authenticated with one of these closes itself when it hears
    /// its own token here.
    closes: broadcast::Sender<String>,
    /// Where the pairings are written so a restart does not forget them.
    store_path: PathBuf,
}

impl CompanionHub {
    /// A hub with whatever was paired last time, or a fresh code and nothing
    /// paired. Fails only if the machine's random source cannot be read, which
    /// is not a state to carry on from: a guessable pairing code is the one
    /// thing this must never have.
    pub fn new(host_name: String, data_dir: PathBuf) -> Result<Self, String> {
        let (events, _) = broadcast::channel(EVENT_BACKLOG);
        let (closes, _) = broadcast::channel(EVENT_BACKLOG);
        let store_path = store::path_in(&data_dir);
        let stored = store::load(&store_path).unwrap_or_default();
        let code = if stored.code.len() == 6 {
            stored.code
        } else {
            rand::pairing_code()?
        };
        // Nothing is connected to a server that has only just started, however
        // the flag was left when the app was last closed.
        let devices = stored
            .devices
            .into_iter()
            .map(|stored| Paired {
                token: stored.token,
                device: Device {
                    connected: false,
                    ..stored.device
                },
                sockets: 0,
                posts: Vec::new(),
            })
            .collect();
        Ok(Self {
            inner: Mutex::new(HubInner {
                code,
                code_at_ms: now_ms(),
                devices,
                lockout: Lockout::default(),
                port: None,
                origins: Vec::new(),
                host_name,
                emit: None,
            }),
            events,
            closes,
            store_path,
        })
    }

    fn lock(&self) -> std::sync::MutexGuard<'_, HubInner> {
        // A panic while the hub was locked would otherwise take the whole
        // companion server down with it for the rest of the session. Nothing
        // under this lock can leave a half-updated invariant behind, so the
        // poisoned guard is the right thing to carry on with.
        self.inner.lock().unwrap_or_else(|e| e.into_inner())
    }

    /// Write the pairings and the code down. Called after anything that
    /// changes either, so a restart picks up where the app left off.
    fn persist(&self) {
        let stored = {
            let inner = self.lock();
            StoredHub {
                code: inner.code.clone(),
                devices: inner
                    .devices
                    .iter()
                    .map(|d| StoredDevice {
                        token: d.token.clone(),
                        device: d.device.clone(),
                    })
                    .collect(),
            }
        };
        store::save(&self.store_path, &stored);
    }

    /// The code on the host's screen, rotated first if it has sat there
    /// unused for [`CODE_MAX_AGE_MS`].
    pub fn code(&self) -> String {
        self.rotate_if_idle(now_ms());
        self.lock().code.clone()
    }

    /// Replace a code nobody has paired with in ten minutes. Existing tokens
    /// are untouched: this changes what a *new* device would have to type,
    /// which is the only thing a code on a screen all afternoon is worth.
    /// Returns whether it rotated, so a test can say so without a wall clock.
    ///
    /// Rotation does not care whether anything is paired. A host with a phone
    /// already on it used to leave the same six digits on screen for the whole
    /// draft, which is the case where somebody else in the room has had the
    /// longest to read them.
    pub fn rotate_if_idle(&self, now: u64) -> bool {
        let stale = {
            let inner = self.lock();
            now.saturating_sub(inner.code_at_ms) >= CODE_MAX_AGE_MS
        };
        if !stale {
            return false;
        }
        let Ok(next) = rand::pairing_code() else {
            // A code that cannot be replaced is still a code; the old one goes
            // on working rather than the host losing its pairing screen.
            return false;
        };
        {
            let mut inner = self.lock();
            inner.code = next;
            inner.code_at_ms = now;
        }
        self.persist();
        // The desktop hears this the same way it hears about devices, and
        // re-reads the status the code is shown from.
        self.publish_devices();
        true
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

    /// The `http://…` origins this server answers on. Empty while it is down.
    pub fn origins(&self) -> Vec<String> {
        self.lock().origins.clone()
    }

    pub fn set_origins(&self, origins: Vec<String>) {
        self.lock().origins = origins;
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
            inner.code_at_ms = now_ms();
            inner.devices.clear();
            inner.lockout.clear();
        }
        self.persist();
        // The sockets themselves are closed by the WebSocket task, which
        // notices its device is gone as soon as it wakes for this frame.
        self.publish_json("revoked", serde_json::json!({}));
        self.publish_devices();
        Ok(code)
    }

    /// Try to pair. The code is compared in constant time and five wrong ones
    /// from one address inside a minute stop that address's sixth from being
    /// tried at all.
    pub fn pair(&self, attempt: PairAttempt<'_>) -> Result<PairOutcome, String> {
        let now = now_ms();
        let token = rand::token()?;
        let fresh_id = rand::device_id()?;
        let mut replaced: Vec<String> = Vec::new();
        let outcome = {
            let mut inner = self.lock();
            if inner.lockout.locked(attempt.peer, now) {
                return Ok(PairOutcome::LockedOut);
            }
            if !rand::secrets_match(attempt.code, &inner.code) {
                inner.lockout.note_failure(attempt.peer, now);
                return Ok(PairOutcome::WrongCode);
            }
            inner.lockout.forgive(attempt.peer);
            let kind = if attempt.kind == "desktop" {
                "desktop"
            } else {
                "phone"
            };
            // Only the same device replaces its own entry. A second phone that
            // happens to also call itself "iPhone" gets "iPhone 2" rather than
            // silently evicting the first one and killing its token.
            let device_id = match attempt.previous_device_id {
                Some(id) if inner.devices.iter().any(|d| d.device.device_id == id) => {
                    replaced.extend(
                        inner
                            .devices
                            .iter()
                            .filter(|d| d.device.device_id == id)
                            .map(|d| d.token.clone()),
                    );
                    inner.devices.retain(|d| d.device.device_id != id);
                    id.to_string()
                }
                _ => fresh_id,
            };
            let taken: Vec<&str> = inner
                .devices
                .iter()
                .map(|d| d.device.name.as_str())
                .collect();
            let name = unique_name(&display_name(attempt.name), &taken);
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
            // A code that has been used is spent: the next device types a new
            // one, so a code glimpsed over a shoulder is worth one pairing.
            if let Ok(next) = rand::pairing_code() {
                inner.code = next;
                inner.code_at_ms = now;
            }
            PairOutcome::Ok {
                token,
                device_id,
                host_name: inner.host_name.clone(),
            }
        };
        // A re-pair leaves the same `device_id` in the list, so nothing else
        // tells the socket the old token opened that it is finished. Without
        // this it went on reading the draft on a token the host has replaced.
        for token in replaced {
            self.close_token(token);
        }
        self.persist();
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

    /// A receiver for tokens that have stopped working. The socket task
    /// listens on this so a token replaced under it closes its connection
    /// rather than leaving the old holder reading on.
    pub fn subscribe_closes(&self) -> broadcast::Receiver<String> {
        self.closes.subscribe()
    }

    /// Tell any socket holding this token that it is finished.
    fn close_token(&self, token: String) {
        let _ = self.closes.send(token);
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

#[cfg(test)]
#[path = "hub_tests.rs"]
mod tests;
