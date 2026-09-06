//! Nothing here runs `/usr/bin/security`. The Keychain side is tested the way
//! `crate::secrets` tests it — by pinning the exact argument list, which is
//! where the mistakes that leak a secret would show up — and the round trips
//! run against a [`FileStore`] in a scratch directory.

use super::*;

/// A directory of this test's own, removed when it is done.
struct Scratch(PathBuf);

impl Scratch {
    fn new(label: &str) -> Self {
        let path = std::env::temp_dir().join(format!(
            "draft-assistant-yahoo-{label}-{}-{}",
            std::process::id(),
            std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .expect("clock")
                .as_nanos()
        ));
        std::fs::create_dir_all(&path).expect("scratch dir");
        Self(path)
    }

    fn store(&self) -> FileStore {
        FileStore::in_dir(&self.0)
    }
}

impl Drop for Scratch {
    fn drop(&mut self) {
        let _ = std::fs::remove_dir_all(&self.0);
    }
}

fn tokens() -> TokenSet {
    TokenSet {
        access_token: "access-1".into(),
        refresh_token: "refresh-1".into(),
        expires_at: 1_700_000_000,
    }
}

fn credentials() -> YahooCredentials {
    YahooCredentials {
        client_id: "dj0yJmk9clientid".into(),
        client_secret: "shhh-secret".into(),
    }
}

#[test]
fn store_builds_an_upserting_add_with_the_value_hex_encoded_in_argv() {
    assert_eq!(
        args_for(Op::Store, Item::Token, Some("{\"a\":1}")),
        [
            "add-generic-password",
            "-U",
            "-s",
            "draft-assistant",
            "-a",
            "yahoo-oauth-token",
            "-w",
            "7b2261223a317d",
        ]
    );
}

/// The stdin route answered the tool's password prompt, which keeps 128
/// bytes and drops the rest without a word. A Yahoo token set and the
/// companion's device list are both longer, and both came back as a stub
/// that would not parse. The value has to travel whole.
#[test]
fn a_value_longer_than_the_password_prompt_keeps_is_passed_whole() {
    let long = format!("{{\"tok\":\"{}\"}}", "x".repeat(6000));
    let args = args_for(Op::Store, Item::Token, Some(&long));
    let sent = args.last().expect("a value");
    assert_eq!(sent.len(), long.len() * 2);
    assert_eq!(decode_stored(sent), long);
}

/// `find-generic-password -w` prints an item as hex once it holds a
/// non-ASCII byte and as text otherwise, so the reader has to take both.
#[test]
fn what_the_tool_prints_decodes_back_to_the_text() {
    let curly = "{\"host\":\"Justin’s MacBook Air\"}";
    assert_eq!(decode_stored(&hex_of(curly)), curly);
    assert_eq!(decode_stored(&format!("{}\n", hex_of("plain"))), "plain");
    // An item written before values were hex-encoded reads as it was.
    assert_eq!(
        decode_stored("{\"client_id\":\"abc\"}"),
        "{\"client_id\":\"abc\"}"
    );
    // Hex that is not UTF-8 is not silently turned into something else.
    assert_eq!(decode_stored("ff"), "ff");
    assert_eq!(decode_stored(""), "");
}

#[test]
fn load_and_clear_name_the_item_the_same_way() {
    assert_eq!(
        args_for(Op::Load, Item::Credentials, None),
        [
            "find-generic-password",
            "-s",
            "draft-assistant",
            "-a",
            "yahoo-app-credentials",
            "-w",
        ]
    );
    assert_eq!(
        args_for(Op::Clear, Item::Credentials, None),
        [
            "delete-generic-password",
            "-s",
            "draft-assistant",
            "-a",
            "yahoo-app-credentials",
        ]
    );
}

/// Every item this store knows about. A new variant added without a line
/// here is a variant nothing below checks, which is how two items end up
/// sharing one Keychain account and overwriting each other.
const ALL_ITEMS: [Item; 3] = [Item::Token, Item::Credentials, Item::CompanionDevices];

#[test]
fn the_items_do_not_share_an_account_with_each_other_or_the_anthropic_key() {
    let mut accounts: Vec<&str> = ALL_ITEMS.iter().map(|item| item.account()).collect();
    accounts.sort_unstable();
    let unique = accounts.len();
    accounts.dedup();
    assert_eq!(accounts.len(), unique, "two items share a Keychain account");
    for item in ALL_ITEMS {
        assert_ne!(item.account(), "anthropic-api-key");
        assert_eq!(crate::secrets::args_for(crate::secrets::Op::Load)[3], "-a");
        assert_ne!(
            crate::secrets::args_for(crate::secrets::Op::Load)[4],
            item.account()
        );
    }
}

#[test]
fn only_a_store_carries_the_value_and_never_as_plain_text() {
    for item in ALL_ITEMS {
        for op in [Op::Load, Op::Clear] {
            let args = args_for(op, item, Some("{\"secret\":\"do-not-send\"}"));
            assert!(
                args.iter().all(|arg| arg.len() < 32),
                "{op:?}/{item:?} carries a value it has no use for: {args:?}"
            );
        }
        let args = args_for(Op::Store, item, Some("{\"secret\":\"do-not-send\"}"));
        assert!(
            args.iter().all(|arg| !arg.contains("do-not-send")),
            "{item:?} puts the text of a secret in argv: {args:?}"
        );
    }
}

#[test]
fn the_companion_item_is_named_under_the_same_service_as_the_rest() {
    assert_eq!(
        args_for(Op::Store, Item::CompanionDevices, Some("{}")),
        [
            "add-generic-password",
            "-U",
            "-s",
            "draft-assistant",
            "-a",
            "companion-devices",
            "-w",
            "7b7d",
        ]
    );
    assert_eq!(
        args_for(Op::Clear, Item::CompanionDevices, None),
        [
            "delete-generic-password",
            "-s",
            "draft-assistant",
            "-a",
            "companion-devices",
        ]
    );
}

#[test]
fn tokens_survive_a_round_trip_through_the_file_store() {
    let scratch = Scratch::new("tokens");
    let store = scratch.store();
    assert!(load_tokens(&store).is_none());
    save_tokens(&store, &tokens()).expect("save");
    assert_eq!(load_tokens(&store), Some(tokens()));
}

#[test]
fn credentials_survive_a_round_trip_through_the_file_store() {
    let scratch = Scratch::new("creds");
    let store = scratch.store();
    save_credentials(&store, &credentials()).expect("save");
    assert_eq!(load_credentials(&store), Some(credentials()));
}

#[test]
fn the_two_items_do_not_overwrite_one_another() {
    let scratch = Scratch::new("both");
    let store = scratch.store();
    save_credentials(&store, &credentials()).expect("save credentials");
    save_tokens(&store, &tokens()).expect("save tokens");
    assert_eq!(load_credentials(&store), Some(credentials()));
    assert_eq!(load_tokens(&store), Some(tokens()));
}

#[test]
fn clearing_forgets_both() {
    let scratch = Scratch::new("clear");
    let store = scratch.store();
    save_credentials(&store, &credentials()).expect("save credentials");
    save_tokens(&store, &tokens()).expect("save tokens");
    clear_all(&store).expect("clear");
    assert!(load_tokens(&store).is_none());
    assert!(load_credentials(&store).is_none());
}

#[test]
fn a_second_save_replaces_the_first() {
    let scratch = Scratch::new("replace");
    let store = scratch.store();
    save_tokens(&store, &tokens()).expect("save");
    let renewed = TokenSet {
        access_token: "access-2".into(),
        ..tokens()
    };
    save_tokens(&store, &renewed).expect("save again");
    assert_eq!(load_tokens(&store), Some(renewed));
}

#[test]
fn half_written_credentials_are_no_credentials() {
    let scratch = Scratch::new("half");
    let store = scratch.store();
    store
        .write(
            Item::Credentials,
            r#"{"client_id":"id","client_secret":""}"#,
        )
        .expect("write");
    assert!(load_credentials(&store).is_none());
}

#[test]
fn a_corrupt_file_reads_as_nothing_stored_rather_than_an_error() {
    let scratch = Scratch::new("corrupt");
    let store = scratch.store();
    std::fs::write(scratch.0.join("yahoo-secrets.json"), "{not json").expect("write");
    assert!(load_tokens(&store).is_none());
    // And writing over it recovers.
    save_tokens(&store, &tokens()).expect("save");
    assert_eq!(load_tokens(&store), Some(tokens()));
}

#[test]
fn a_stored_value_that_is_not_a_token_reads_as_nothing() {
    let scratch = Scratch::new("junk");
    let store = scratch.store();
    store
        .write(Item::Token, "not a token at all")
        .expect("write");
    assert!(load_tokens(&store).is_none());
}

#[cfg(unix)]
#[test]
fn the_fallback_file_is_readable_only_by_its_owner() {
    use std::os::unix::fs::PermissionsExt;
    let scratch = Scratch::new("perms");
    let store = scratch.store();
    save_tokens(&store, &tokens()).expect("save");
    let mode = std::fs::metadata(scratch.0.join("yahoo-secrets.json"))
        .expect("metadata")
        .permissions()
        .mode();
    assert_eq!(mode & 0o077, 0, "mode {mode:o} lets someone else read it");
}

#[test]
fn signing_out_forgets_the_token_and_keeps_the_registered_app() {
    // Disconnect is signing out of an account, not un-registering the app: the
    // client id and secret cost a trip to developer.yahoo.com to replace, so
    // reconnecting has to stay one click.
    let scratch = Scratch::new("signout");
    let store = scratch.store();
    save_credentials(&store, &credentials()).expect("save credentials");
    save_tokens(&store, &tokens()).expect("save tokens");
    clear_tokens(&store).expect("clear the token");
    assert!(load_tokens(&store).is_none());
    assert_eq!(load_credentials(&store), Some(credentials()));
    // And it is idempotent: signing out twice is not an error.
    clear_tokens(&store).expect("clear again");
    assert_eq!(load_credentials(&store), Some(credentials()));
}

#[test]
fn a_machine_without_a_keychain_still_gets_a_store_that_works() {
    // `store_for` is the only place that decides between the two backends. On
    // a machine with no `security` binary it has to hand back the file store
    // rather than a `Keychain` that would fail on every call — and whichever
    // one this machine gets, the round trip has to work.
    let scratch = Scratch::new("store-for");
    let store = store_for(&scratch.0);
    if available() {
        // The Keychain is this developer's real login keychain; writing to it
        // from a test is exactly what these tests must not do.
        return;
    }
    save_tokens(store.as_ref(), &tokens()).expect("save through the fallback");
    assert_eq!(load_tokens(store.as_ref()), Some(tokens()));
    assert!(
        scratch.0.join("yahoo-secrets.json").is_file(),
        "the fallback did not write to the data directory"
    );
}

#[test]
fn a_directory_that_does_not_exist_yet_is_created_on_first_write() {
    let scratch = Scratch::new("nested");
    let store = FileStore::in_dir(scratch.0.join("does").join("not").join("exist"));
    save_tokens(&store, &tokens()).expect("save into a fresh directory");
    assert_eq!(load_tokens(&store), Some(tokens()));
}
