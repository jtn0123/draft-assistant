//! Where the companion remembers who is paired, between runs of the app.
//!
//! Without this file every restart of the host silently unpaired every phone:
//! the tokens only ever lived in memory, so a phone that woke up with a token
//! in `localStorage` was told "not paired" by a host that had simply forgotten
//! it. What is written here is a secret — the device tokens are bearer tokens
//! for the whole read API — so the file is owner-only and nothing in it is
//! ever logged.

use super::hub::Device;
use serde::{Deserialize, Serialize};
use std::path::{Path, PathBuf};

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

/// The file the hub is written to, inside the app's data directory.
pub fn path_in(data_dir: &Path) -> PathBuf {
    data_dir.join("companion_devices.json")
}

/// What was written last time, or nothing. A file that will not parse is
/// treated as absent rather than as an error: the cost is re-pairing, and
/// refusing to start the app over it would be worse.
pub fn load(path: &Path) -> Option<StoredHub> {
    let raw = std::fs::read_to_string(path).ok()?;
    serde_json::from_str::<StoredHub>(&raw).ok()
}

/// Write the pairings down, owner-only, through a temp file.
///
/// A failed write is logged without any of its content and otherwise ignored:
/// the pairing the user just made is already live in memory, and losing it at
/// the next restart is not a reason to refuse it now.
pub fn save(path: &Path, stored: &StoredHub) {
    if let Some(parent) = path.parent() {
        if std::fs::create_dir_all(parent).is_ok() {
            crate::cache::owner_only_dir(parent);
        }
    }
    let Ok(json) = serde_json::to_string(stored) else {
        crate::applog::warn("could not prepare the paired devices to be saved");
        return;
    };
    let tmp = crate::cache::temp_sibling(path);
    if crate::cache::replace_file(tmp, path.to_path_buf(), json).is_err() {
        // Deliberately not the error text: it carries the path, and the path
        // is the one place on disk the tokens live.
        crate::applog::warn("could not save the paired devices");
    }
}

#[cfg(test)]
mod tests {
    use super::{load, path_in, save, StoredDevice, StoredHub};
    use crate::companion::hub::Device;

    fn dir(label: &str) -> std::path::PathBuf {
        let dir = std::env::temp_dir().join(format!(
            "companion-store-{label}-{}-{:?}",
            std::process::id(),
            std::time::SystemTime::now()
        ));
        std::fs::create_dir_all(&dir).expect("the scratch directory is creatable");
        dir
    }

    fn sample() -> StoredHub {
        StoredHub {
            code: "424242".to_string(),
            devices: vec![StoredDevice {
                token: "tok".to_string(),
                device: Device {
                    device_id: "dev".to_string(),
                    name: "Rob's iPhone".to_string(),
                    kind: "phone".to_string(),
                    paired_at_ms: 7,
                    last_seen_ms: 8,
                    connected: true,
                },
            }],
        }
    }

    #[test]
    /// Only the round trip. Whether a restored device counts as connected is
    /// the hub's business, not the file's -- the hub clears the flag when it
    /// reads this back, and `hub_tests` is where that is asserted.
    fn every_field_of_a_paired_device_survives_the_round_trip() {
        let path = path_in(&dir("roundtrip"));
        assert!(load(&path).is_none(), "nothing has been written yet");
        save(&path, &sample());
        let back = load(&path).expect("the file reads back");
        assert_eq!(back.code, "424242");
        assert_eq!(back.devices.len(), 1);
        assert_eq!(back.devices[0].token, "tok");
        let device = &back.devices[0].device;
        assert_eq!(device.device_id, "dev");
        assert_eq!(device.name, "Rob's iPhone");
        assert_eq!(device.kind, "phone");
        assert_eq!(device.paired_at_ms, 7);
        assert_eq!(device.last_seen_ms, 8);
        // The file records what was true when it was written; nothing here
        // reinterprets it.
        assert!(device.connected);
    }

    #[test]
    fn the_file_holding_the_tokens_is_readable_only_by_its_owner() {
        let path = path_in(&dir("mode"));
        save(&path, &sample());
        #[cfg(unix)]
        {
            use std::os::unix::fs::PermissionsExt;
            let mode = std::fs::metadata(&path)
                .expect("the file exists")
                .permissions();
            assert_eq!(mode.mode() & 0o777, 0o600, "the token file is not private");
        }
    }

    #[test]
    fn a_file_that_will_not_parse_is_treated_as_nobody_being_paired() {
        let path = path_in(&dir("corrupt"));
        std::fs::write(&path, "{ not json").expect("the file writes");
        assert!(load(&path).is_none());
    }
}
