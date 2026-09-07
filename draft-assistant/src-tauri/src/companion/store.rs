//! Where the companion remembers who is paired, between runs of the app.
//!
//! Without this the app silently unpaired every phone on restart: the tokens
//! only ever lived in memory, so a phone that woke up with a token in
//! `localStorage` was told "not paired" by a host that had simply forgotten
//! it.
//!
//! What is kept here is a secret. A device token is a bearer token for the
//! whole read API and the pairing code is what turns a stranger on the LAN
//! into a paired device, so both go in the machine's Keychain through
//! [`crate::yahoo_secrets`] rather than into a file. They used to sit in a
//! plaintext `companion_devices.json`, owner-only but still readable by
//! anything running as the user; that file is now migrated into the store the
//! first time it is seen and deleted. Nothing in here is ever logged.

use super::hub::Device;
use crate::yahoo_secrets::{Item, SecretStore};
use serde::{Deserialize, Serialize};
use std::path::{Path, PathBuf};

/// The store the hub keeps its pairings in, filed under one named item.
///
/// The desktop app and the headless `companion_host` on one Mac used to
/// share `companion-devices`: pairing a phone against the host overwrote
/// every phone paired to the desktop, and the desktop's next save overwrote
/// the host's. Each process now says which item is its own when it builds
/// its server, and this wrapper files every device-list read and write
/// under that item, so the hub itself never has to know which process it is
/// in. Any other item passes straight through.
pub struct DevicesUnder {
    inner: Box<dyn SecretStore>,
    item: Item,
}

impl DevicesUnder {
    /// `inner`, with the device list filed under `item`. Anything but
    /// [`Item::CompanionDevices`] or [`Item::CompanionDevicesHeadless`] is
    /// refused: the list has to go under one of the two companion accounts,
    /// never over the Yahoo tokens or the API key.
    pub fn new(inner: Box<dyn SecretStore>, item: Item) -> Result<Self, String> {
        match item {
            Item::CompanionDevices | Item::CompanionDevicesHeadless => Ok(Self { inner, item }),
            other => Err(format!(
                "the paired devices cannot be filed under {}",
                other.account()
            )),
        }
    }

    fn map(&self, item: Item) -> Item {
        if item == DEVICES {
            self.item
        } else {
            item
        }
    }
}

impl SecretStore for DevicesUnder {
    fn read(&self, item: Item) -> Option<String> {
        self.inner.read(self.map(item))
    }

    fn write(&self, item: Item, value: &str) -> Result<(), String> {
        self.inner.write(self.map(item), value)
    }

    fn clear(&self, item: Item) -> Result<(), String> {
        self.inner.clear(self.map(item))
    }
}

/// The item [`load`] and [`save`] name. A [`DevicesUnder`] around the store
/// is what turns it into the headless host's account when that is wanted.
const DEVICES: Item = Item::CompanionDevices;

/// One paired device as it survives a restart: the device the contract
/// describes, plus the token that device authenticates with.
#[derive(Clone, Serialize, Deserialize)]
pub struct StoredDevice {
    pub token: String,
    pub device: Device,
}

/// Written by hand rather than derived, for the reason
/// [`crate::yahoo_oauth::TokenSet`]'s is: a device token is a bearer token for
/// the whole read API, and a derived `Debug` puts it into any `{:?}` — one
/// `dbg!` in a panic message or a log line is all it would take to spill it.
/// The device it belongs to stays, because that is the field a failing test
/// actually wants to see.
impl std::fmt::Debug for StoredDevice {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("StoredDevice")
            .field("token", &"<redacted>")
            .field("device", &self.device)
            .finish()
    }
}

/// The whole of what the hub carries across a restart.
#[derive(Default, Serialize, Deserialize)]
pub struct StoredHub {
    /// The six digits currently on the host's screen. Kept so a code read off
    /// the Mac a moment before a crash still works after it.
    #[serde(default)]
    pub code: String,
    /// When that code was minted, in epoch milliseconds.
    ///
    /// Kept beside the code because the code alone is not enough to know
    /// whether it is still worth honouring. A restart used to stamp whatever
    /// was in the store as freshly minted, so six digits somebody wrote down
    /// weeks ago were live again for ten minutes after every launch. A store
    /// written before this field existed reads as 0, which is older than any
    /// window and so is replaced rather than trusted.
    #[serde(default)]
    pub code_at_ms: u64,
    #[serde(default)]
    pub devices: Vec<StoredDevice>,
}

/// Also by hand: the pairing code is what turns a stranger on the LAN into a
/// paired device, so it is no more printable than the tokens beside it.
impl std::fmt::Debug for StoredHub {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("StoredHub")
            .field("code", &"<redacted>")
            .field("devices", &self.devices)
            .finish()
    }
}

/// The plaintext file older builds wrote. Nothing writes it any more; it is
/// only looked for once, so what an upgrading user had paired is carried over.
pub fn legacy_path_in(data_dir: &Path) -> PathBuf {
    data_dir.join("companion_devices.json")
}

/// What was stored last time, or nothing.
///
/// A value that will not parse is treated as absent rather than as an error:
/// the cost is re-pairing, and refusing to start the app over it would be
/// worse.
pub fn load(store: &dyn SecretStore, data_dir: &Path) -> Option<StoredHub> {
    load_item(store, data_dir, DEVICES)
}

/// [`load`] against a named item, so a test can show the desktop's and the
/// headless host's lists never read as each other's.
pub fn load_item(store: &dyn SecretStore, data_dir: &Path, item: Item) -> Option<StoredHub> {
    if let Some(raw) = store.read(item) {
        return serde_json::from_str::<StoredHub>(&raw).ok();
    }
    migrate_legacy_file(store, data_dir, item)
}

/// Move a pre-Keychain `companion_devices.json` into the store and delete it.
///
/// The file is removed even when it will not parse: it cannot be used for
/// anything, and leaving a file of bearer tokens on disk is the failure this
/// whole module exists to end. It is kept only when the store refused the
/// write, so that a later run can try the move again instead of unpairing
/// every phone.
fn migrate_legacy_file(store: &dyn SecretStore, data_dir: &Path, item: Item) -> Option<StoredHub> {
    let path = legacy_path_in(data_dir);
    let raw = std::fs::read_to_string(&path).ok()?;
    let Ok(stored) = serde_json::from_str::<StoredHub>(&raw) else {
        let _ = std::fs::remove_file(&path);
        return None;
    };
    if write(store, &stored, item).is_err() {
        crate::applog::warn("could not move the paired devices into the keychain");
        return Some(stored);
    }
    if std::fs::remove_file(&path).is_err() {
        crate::applog::warn("could not delete the old paired devices file");
    }
    Some(stored)
}

/// Put the pairings and the code in the store.
///
/// A failed write is logged without any of its content and otherwise ignored:
/// the pairing the user just made is already live in memory, and losing it at
/// the next restart is not a reason to refuse it now.
pub fn save(store: &dyn SecretStore, stored: &StoredHub) {
    save_item(store, stored, DEVICES);
}

/// [`save`] against a named item.
pub fn save_item(store: &dyn SecretStore, stored: &StoredHub, item: Item) {
    if write(store, stored, item).is_err() {
        // Deliberately not the error text: a store error can quote the value
        // it was handed, and that value is every paired phone's token.
        crate::applog::warn("could not save the paired devices");
    }
}

fn write(store: &dyn SecretStore, stored: &StoredHub, item: Item) -> Result<(), ()> {
    let json = serde_json::to_string(stored).map_err(|_| ())?;
    store.write(item, &json).map_err(|_| ())
}

#[cfg(test)]
#[path = "store_tests.rs"]
mod tests;
