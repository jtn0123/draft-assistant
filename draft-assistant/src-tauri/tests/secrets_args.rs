//! Keychain argument construction only. Nothing here runs /usr/bin/security.

use draft_assistant_lib::secrets::{args_for, available, Op};

#[test]
fn store_builds_an_upserting_add_command_that_reads_the_key_from_stdin() {
    // No key in argv: `-w` with no value makes `security` read it from stdin,
    // which keeps it out of `ps` output.
    assert_eq!(
        args_for(Op::Store),
        [
            "add-generic-password",
            "-U",
            "-s",
            "draft-assistant",
            "-a",
            "anthropic-api-key",
            "-w",
        ]
    );
}

#[test]
fn no_operation_can_put_a_secret_in_the_argument_list() {
    for op in [Op::Store, Op::Load, Op::Clear] {
        let args = args_for(op);
        assert!(
            args.iter().all(|a| !a.starts_with("sk-")),
            "{op:?} leaked a key: {args:?}"
        );
    }
}

#[test]
fn load_requests_only_the_password() {
    assert_eq!(
        args_for(Op::Load),
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
        args_for(Op::Clear),
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
fn availability_tracks_the_security_binary_on_macos() {
    let expected = cfg!(target_os = "macos") && std::path::Path::new("/usr/bin/security").is_file();
    assert_eq!(available(), expected);
}
