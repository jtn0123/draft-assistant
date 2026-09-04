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
fn store_builds_an_upserting_add_that_reads_the_value_from_stdin() {
    assert_eq!(
        args_for(Op::Store, Item::Token),
        [
            "add-generic-password",
            "-U",
            "-s",
            "draft-assistant",
            "-a",
            "yahoo-oauth-token",
            "-w",
        ]
    );
}

#[test]
fn load_and_clear_name_the_item_the_same_way() {
    assert_eq!(
        args_for(Op::Load, Item::Credentials),
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
        args_for(Op::Clear, Item::Credentials),
        [
            "delete-generic-password",
            "-s",
            "draft-assistant",
            "-a",
            "yahoo-app-credentials",
        ]
    );
}

#[test]
fn the_two_items_do_not_share_an_account_with_each_other_or_the_anthropic_key() {
    assert_ne!(Item::Token.account(), Item::Credentials.account());
    for item in [Item::Token, Item::Credentials] {
        assert_ne!(item.account(), "anthropic-api-key");
        assert_eq!(crate::secrets::args_for(crate::secrets::Op::Load)[3], "-a");
        assert_ne!(
            crate::secrets::args_for(crate::secrets::Op::Load)[4],
            item.account()
        );
    }
}

#[test]
fn no_operation_can_put_a_value_in_the_argument_list() {
    for op in [Op::Store, Op::Load, Op::Clear] {
        for item in [Item::Token, Item::Credentials] {
            let args = args_for(op, item);
            assert!(
                args.iter().all(|arg| arg.len() < 32),
                "{op:?}/{item:?} has an argument long enough to be a token: {args:?}"
            );
        }
    }
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
