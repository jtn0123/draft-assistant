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
use std::sync::atomic::{AtomicBool, Ordering};

/// Whether this process is the headless `companion_host` rather than the
/// desktop app. Set once, before the hub is built, and never cleared.
///
/// The choice lives here rather than in the server's constructor because the
/// server builds its hub, and the hub its store, without a parameter for it;
/// what differs between the two is only which Keychain account the device
/// list goes under, and this module is the one place that names it.
static HEADLESS: AtomicBool = AtomicBool::new(false);

/// Keep this process's pairings apart from the desktop app's. The headless
/// host and the desktop app on one Mac used to share `companion-devices`:
/// pairing a phone against the host overwrote every phone paired to the
/// desktop, and the desktop's next save overwrote the host's. Called by
/// `companion_host` before it builds its server.
pub fn select_headless_account() {
    HEADLESS.store(true, Ordering::SeqCst);
}

/// The item the device list of this process goes under.
pub fn devices_item() -> Item {
    devices_item_for(HEADLESS.load(Ordering::SeqCst))
}

/// The pure half of [`devices_item`]: which item a headless or a desktop
/// process keeps its pairings in.
pub fn devices_item_for(headless: bool) -> Item {
    if headless {
        Item::CompanionDevicesHeadless
    } else {
        Item::CompanionDevices
    }
}

/// One paired device as it survives a restart: the device the contract
/// describes, plus the token that device authenticates with.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct StoredDevice {
    pub token: String,
    pub device: Device,
}

/// The whole of what the hub carries across a restart.
#[derive(Debug, Default, Serialize, Deserialize)]
pub struct StoredHub {
    /// The six digits currently on the host's screen. Kept so a code read off
    /// the Mac a moment before a crash still works after it.
    #[serde(default)]
    pub code: String,
    #[serde(default)]
    pub devices: Vec<StoredDevice>,
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
    load_item(store, data_dir, devices_item())
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
    save_item(store, stored, devices_item());
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
