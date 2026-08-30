//! Keychain argument construction only. Nothing here runs /usr/bin/security.

use draft_assistant_lib::secrets::{args_for, available};

#[test]
fn store_builds_an_upserting_add_command() {
    assert_eq!(
        args_for("store", Some("sk-ant-test")),
        [
            "add-generic-password",
            "-U",
            "-s",
            "draft-assistant",
            "-a",
            "anthropic-api-key",
            "-w",
            "sk-ant-test",
        ]
    );
}

#[test]
fn store_without_a_key_omits_the_password_flag() {
    let args = args_for("store", None);
    assert!(!args.contains(&"-w".to_string()));
}

#[test]
fn load_requests_only_the_password() {
    assert_eq!(
        args_for("load", None),
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
fn load_ignores_a_stray_key_argument() {
    assert_eq!(args_for("load", Some("ignored")), args_for("load", None));
}

#[test]
fn clear_deletes_by_service_and_account_without_a_password() {
    assert_eq!(
        args_for("clear", None),
        [
            "delete-generic-password",
            "-s",
            "draft-assistant",
            "-a",
            "anthropic-api-key",
        ]
    );
}

#[test]
#[should_panic(expected = "unknown keychain op")]
fn unknown_ops_are_a_programming_error() {
    args_for("frobnicate", None);
}

#[test]
fn availability_tracks_the_security_binary_on_macos() {
    let expected = cfg!(target_os = "macos") && std::path::Path::new("/usr/bin/security").is_file();
    assert_eq!(available(), expected);
}
