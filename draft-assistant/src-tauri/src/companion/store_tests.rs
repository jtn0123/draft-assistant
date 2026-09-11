//! Tests for the paired-device store. Every one of them runs against
//! [`FileStore`] in a scratch directory: nothing here may write to the
//! developer's real login Keychain.

use super::{
    legacy_path_in, load, load_item, save, save_item, DevicesUnder, StoredDevice, StoredHub,
};
use crate::companion::hub::Device;
use crate::yahoo_secrets::{FileStore, Item, SecretStore};
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
        code_at_ms: 9,
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
    save(scratch.store.as_ref(), &sample()).unwrap();
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
/// What an upgrading user gets: the phone paired against the old build stays
/// paired, and the plaintext file it was paired through is gone for good.
fn an_old_plaintext_file_is_moved_into_the_store_and_no_token_or_code_stays_on_disk() {
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
    // The failure this prevents: a device token, which is a bearer token for
    // the whole read API, and the code, left in any file under the data
    // directory anything running as the user can read. This walk is the one
    // that can fail: the migration is the only path that ever wrote there.
    assert!(
        !contains_anywhere(&scratch.data_dir, "tok"),
        "a device token is still on disk in the data directory"
    );
    assert!(
        !contains_anywhere(&scratch.data_dir, "424242"),
        "the pairing code is still on disk in the data directory"
    );
    // A later save must not bring the file back either.
    let mut changed = sample();
    changed.code = "535353".to_string();
    save(scratch.store.as_ref(), &changed).unwrap();
    assert!(!path.exists(), "a save recreated the plaintext file");
    assert!(!contains_anywhere(&scratch.data_dir, "535353"));

    // The second construction is the restart after the upgrade: there is no
    // file left to read it out of, so it has to come back from the store,
    // as the last save left it.
    let again = load(scratch.store.as_ref(), &scratch.data_dir).expect("the store kept it");
    assert_eq!(again.code, "535353");
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

#[test]
/// The failure this prevents: the headless host and the desktop app on one
/// Mac kept their pairings under one account, so pairing a phone against the
/// host replaced every phone paired to the desktop, and the other way round.
/// Same store, two items: a save under one is invisible to a load under the
/// other, and a second save does not disturb the first.
fn the_headless_host_and_the_desktop_do_not_read_each_others_pairings() {
    let scratch = scratch("two-accounts");
    let desktop = Item::CompanionDevices;
    let headless = Item::CompanionDevicesHeadless;

    save_item(scratch.store.as_ref(), &sample(), desktop).unwrap();
    assert!(
        load_item(scratch.store.as_ref(), &scratch.data_dir, headless).is_none(),
        "the headless host read the desktop's pairings"
    );

    let mut hosts = sample();
    hosts.code = "919191".to_string();
    hosts.devices[0].token = "host-tok".to_string();
    save_item(scratch.store.as_ref(), &hosts, headless).unwrap();

    let desk = load_item(scratch.store.as_ref(), &scratch.data_dir, desktop).expect("desktop");
    assert_eq!(desk.code, "424242");
    assert_eq!(desk.devices[0].token, "tok");
    let host = load_item(scratch.store.as_ref(), &scratch.data_dir, headless).expect("host");
    assert_eq!(host.code, "919191");
    assert_eq!(host.devices[0].token, "host-tok");
}

/// The store the headless host's hub is built over: a plain `load` and `save`
/// through it land under the headless account, and a hub over the same
/// underlying store filed under the desktop account sees none of it.
#[test]
fn a_store_filed_under_the_headless_account_keeps_the_hubs_plain_saves_apart() {
    let root = dir("under");
    let data_dir = root.join("data");
    std::fs::create_dir_all(&data_dir).expect("the data directory is creatable");
    let shared = root.join("secrets");
    let headless = DevicesUnder::new(
        Box::new(FileStore::in_dir(&shared)),
        Item::CompanionDevicesHeadless,
    )
    .expect("a companion account");
    let desktop = DevicesUnder::new(Box::new(FileStore::in_dir(&shared)), Item::CompanionDevices)
        .expect("a companion account");

    save(&headless, &sample()).unwrap();
    assert!(
        load(&desktop, &data_dir).is_none(),
        "the desktop read the headless host's pairings"
    );
    let raw = FileStore::in_dir(&shared);
    assert!(raw.read(Item::CompanionDevices).is_none());
    assert!(raw.read(Item::CompanionDevicesHeadless).is_some());
    assert_eq!(load(&headless, &data_dir).expect("kept").code, "424242");

    // Anything that is not the device list passes through untouched.
    headless
        .write(Item::Token, "not-a-real-token")
        .expect("written");
    assert_eq!(raw.read(Item::Token).as_deref(), Some("not-a-real-token"));
    headless.clear(Item::Token).expect("cleared");
    assert!(raw.read(Item::Token).is_none());
}

/// The device list only ever goes under one of the two companion accounts.
#[test]
fn the_device_list_is_refused_a_home_over_any_other_secret() {
    let root = dir("refused");
    for item in [Item::Credentials, Item::Token, Item::AnthropicKey] {
        let error = DevicesUnder::new(Box::new(FileStore::in_dir(root.join("secrets"))), item)
            .err()
            .expect("refused");
        assert!(error.contains(item.account()), "{error}");
    }
}

/// A device token is a bearer token for the whole read API, and a derived
/// `Debug` puts it one `{:?}` away from the log. The Yahoo token pair is
/// written by hand for the same reason; this is the same treatment.
#[test]
fn a_device_token_and_the_pairing_code_cannot_be_printed_by_accident() {
    let mut stored = sample();
    stored.devices[0].token = "s3cret-bearer".to_string();
    let printed = format!("{stored:?}");
    assert!(!printed.contains("s3cret-bearer"), "{printed}");
    assert!(!printed.contains("424242"), "{printed}");
    assert!(printed.contains("<redacted>"), "{printed}");
    // The device itself is still there to debug with.
    assert!(printed.contains("Rob's iPhone"), "{printed}");
    let one = format!("{:?}", stored.devices[0]);
    assert!(!one.contains("s3cret-bearer"), "{one}");
    assert!(one.contains("<redacted>"), "{one}");
}
