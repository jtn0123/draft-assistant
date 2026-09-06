//! Keychain argument construction only. Nothing here runs /usr/bin/security.

use draft_assistant_lib::secrets::{args_for, available, Op};
use draft_assistant_lib::yahoo_secrets::hex_of;

const KEY: &str = "sk-ant-api03-not-a-real-key";

#[test]
fn store_builds_an_upserting_add_command_with_the_key_hex_encoded_in_argv() {
    // Not the stdin password prompt: that keeps 128 bytes and drops the rest
    // without a word. Hex rather than the text, so `find-generic-password -w`
    // prints back plain ASCII whatever the key holds and `ps` never shows the
    // key as written.
    assert_eq!(
        args_for(Op::Store, Some(KEY)),
        [
            "add-generic-password",
            "-U",
            "-s",
            "draft-assistant",
            "-a",
            "anthropic-api-key",
            "-w",
            &hex_of(KEY),
        ]
    );
}

#[test]
fn no_operation_can_put_a_secret_in_the_argument_list_as_text() {
    for op in [Op::Store, Op::Load, Op::Clear] {
        let args = args_for(op, Some(KEY));
        assert!(
            args.iter().all(|a| !a.contains("sk-")),
            "{op:?} leaked a key: {args:?}"
        );
    }
}

#[test]
fn load_requests_only_the_password() {
    assert_eq!(
        args_for(Op::Load, None),
        [
            "find-generic-password",
            "-s",
            "draft-assistant",
            "-a",
            "anthropic-api-key",
            "-w",
        ]
    );
}

#[test]
fn clear_deletes_by_service_and_account_without_a_password() {
    assert_eq!(
        args_for(Op::Clear, None),
        [
            "delete-generic-password",
            "-s",
            "draft-assistant",
            "-a",
            "anthropic-api-key",
        ]
    );
}

/// `available()` is a promise about the Keychain store: everywhere it says
/// yes, the app goes on to spawn `/usr/bin/security` by that exact hardcoded
/// path. The previous test here recomputed the function's own body and
/// compared the two, which passed for any body at all -- including one that
/// named a path nothing spawns. This checks the coupling instead.
#[test]
fn saying_the_keychain_is_available_means_the_binary_it_will_spawn_is_really_there() {
    if !available() {
        // Nothing is claimed, so nothing is promised. Asserting the negative
        // would just be the old restatement in reverse.
        return;
    }
    let security = std::path::Path::new("/usr/bin/security");
    assert!(
        security.is_file(),
        "available() said yes but {} is not there for run() to spawn",
        security.display()
    );
}

/// The other half of the same promise, and the fallback the rest of the app
/// depends on: off macOS there is no Keychain to ask, so `available()` must be
/// false and `Engine::api_key` must keep reading the key out of the config
/// file. Written as a `cfg`-gated test rather than an `assert!(cfg!(..))`,
/// which is a constant the compiler folds away rather than a check.
#[test]
#[cfg(not(target_os = "macos"))]
fn a_non_macos_build_never_reaches_for_the_keychain() {
    assert!(
        !available(),
        "available() opted a non-macOS build into spawning /usr/bin/security"
    );
}
