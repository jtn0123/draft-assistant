//! Tests for the paired-device store. Every one of them runs against
//! [`FileStore`] in a scratch directory: nothing here may write to the
//! developer's real login Keychain.

use super::{legacy_path_in, load, save, StoredDevice, StoredHub};
use crate::companion::hub::Device;
use crate::yahoo_secrets::{FileStore, SecretStore};
use std::path::{Path, PathBuf};

fn dir(label: &str) -> PathBuf {
    let dir = std::env::temp_dir().join(format!(
        "companion-store-{label}-{}-{:?}",
        std::process::id(),
        std::time::SystemTime::now()
    ));
    std::fs::create_dir_all(&dir).expect("the scratch directory is creatable");
    dir
}

/// A data directory and, beside it, the directory the stand-in secret store
/// keeps its own file in. They are kept apart so a test can walk the data
/// directory and say that no token is in any file there, which is the whole
/// claim this module makes on a machine with a Keychain.
struct Scratch {
    data_dir: PathBuf,
    store: Box<dyn SecretStore>,
}

fn scratch(label: &str) -> Scratch {
    let root = dir(label);
    let data_dir = root.join("data");
    std::fs::create_dir_all(&data_dir).expect("the data directory is creatable");
    Scratch {
        store: Box::new(FileStore::in_dir(root.join("secrets"))),
        data_dir,
    }
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

fn contains_anywhere(dir: &Path, needle: &str) -> bool {
    let Ok(entries) = std::fs::read_dir(dir) else {
        return false;
    };
    entries.flatten().any(|entry| {
        let path = entry.path();
        if path.is_dir() {
            return contains_anywhere(&path, needle);
        }
        std::fs::read(&path)
            .map(|bytes| String::from_utf8_lossy(&bytes).contains(needle))
            .unwrap_or(false)
    })
}

#[test]
/// Only the round trip. Whether a restored device counts as connected is
/// the hub's business, not the store's -- the hub clears the flag when it
/// reads this back, and `hub_tests` is where that is asserted.
fn every_field_of_a_paired_device_survives_the_round_trip() {
    let scratch = scratch("roundtrip");
    assert!(
        load(scratch.store.as_ref(), &scratch.data_dir).is_none(),
        "nothing has been written yet"
    );
    save(scratch.store.as_ref(), &sample());
    let back = load(scratch.store.as_ref(), &scratch.data_dir).expect("the store reads back");
    assert_eq!(back.code, "424242");
    assert_eq!(back.devices.len(), 1);
    assert_eq!(back.devices[0].token, "tok");
    let device = &back.devices[0].device;
    assert_eq!(device.device_id, "dev");
    assert_eq!(device.name, "Rob's iPhone");
    assert_eq!(device.kind, "phone");
    assert_eq!(device.paired_at_ms, 7);
    assert_eq!(device.last_seen_ms, 8);
    // The store records what was true when it was written; nothing here
    // reinterprets it.
    assert!(device.connected);
}

#[test]
/// The failure this prevents: a device token, which is a bearer token for the
/// whole read API, sitting in a file anything running as the user can read.
fn saving_puts_no_token_or_code_in_a_file_in_the_data_directory() {
    let scratch = scratch("no-plaintext");
    save(scratch.store.as_ref(), &sample());
    assert!(
        !contains_anywhere(&scratch.data_dir, "tok"),
        "a device token was written into the data directory"
    );
    assert!(
        !contains_anywhere(&scratch.data_dir, "424242"),
        "the pairing code was written into the data directory"
    );
}

#[test]
/// What an upgrading user gets: the phone paired against the old build stays
/// paired, and the plaintext file it was paired through is gone for good.
fn an_old_plaintext_file_is_moved_into_the_store_and_deleted() {
    let scratch = scratch("migrate");
    let path = legacy_path_in(&scratch.data_dir);
    std::fs::write(
        &path,
        serde_json::to_string(&sample()).expect("the sample serialises"),
    )
    .expect("the old file writes");

    let first = load(scratch.store.as_ref(), &scratch.data_dir).expect("the old file is read");
    assert_eq!(first.devices[0].token, "tok");
    assert!(!path.exists(), "the plaintext file outlived the migration");
    assert!(!contains_anywhere(&scratch.data_dir, "tok"));

    // The second construction is the restart after the upgrade: there is no
    // file left to read it out of, so it has to come back from the store.
    let again = load(scratch.store.as_ref(), &scratch.data_dir).expect("the store kept it");
    assert_eq!(again.code, "424242");
    assert_eq!(again.devices[0].token, "tok");
}

#[test]
fn a_stored_value_that_will_not_parse_is_treated_as_nobody_being_paired() {
    let scratch = scratch("corrupt");
    scratch
        .store
        .write(crate::yahoo_secrets::Item::CompanionDevices, "{ not json")
        .expect("the stand-in store writes");
    assert!(load(scratch.store.as_ref(), &scratch.data_dir).is_none());
}

#[test]
fn an_old_file_that_will_not_parse_is_deleted_rather_than_left_lying_there() {
    let scratch = scratch("corrupt-file");
    let path = legacy_path_in(&scratch.data_dir);
    std::fs::write(&path, "{ not json").expect("the file writes");
    assert!(load(scratch.store.as_ref(), &scratch.data_dir).is_none());
    assert!(!path.exists(), "an unreadable token file was left on disk");
}
