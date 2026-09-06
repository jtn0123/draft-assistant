//! The pure half of Settings -> "Check for updates": what the row says for
//! each way the updater can fail, and the outcome shape a check produces.

use super::{describe, outcome, UpdateCheck};
use tauri_plugin_updater::Error;

/// A fresh install checks before the first signed release exists, so
/// `latest.json` is a 404 and the plugin says "Could not fetch a valid
/// release JSON from the remote". The row must say there is no feed yet, not
/// hand a developer sentence to the user.
#[test]
fn a_missing_feed_reads_as_no_release_yet_not_as_json_trouble() {
    let text = describe(&Error::ReleaseNotFound);
    assert!(text.starts_with("No release feed yet"), "{text}");
    assert!(!text.contains("JSON"), "{text}");
}

/// Offline, the plugin surfaces a transport error whose text names the
/// endpoint and the socket. The row says the server could not be reached.
#[test]
fn a_transport_failure_says_the_server_could_not_be_reached() {
    let text = describe(&Error::Network("connection refused (os error 61)".into()));
    assert!(
        text.starts_with("Could not reach the update server"),
        "{text}"
    );
    assert!(!text.contains("os error"), "{text}");
}

/// A signature mismatch is the one failure that must never read as "try
/// again": the archive was refused on purpose, and the sentence says so.
#[test]
fn a_bad_signature_says_the_download_was_refused() {
    let text = describe(&Error::SignatureUtf8("not base64".into()));
    assert!(text.contains("did not match its signature"), "{text}");
    assert!(text.contains("not installed"), "{text}");
}

#[test]
fn a_release_with_no_build_for_this_mac_says_so() {
    let text = describe(&Error::TargetNotFound("darwin-aarch64".into()));
    assert_eq!(text, "The latest release has no build for this Mac");
}

#[test]
fn a_build_with_no_endpoints_says_it_cannot_check() {
    assert!(describe(&Error::EmptyEndpoints).contains("no update feed configured"));
}

#[test]
fn a_disk_failure_points_at_the_disk() {
    let error = Error::Io(std::io::Error::other("No space left on device"));
    assert!(describe(&error).starts_with("Could not write the update to disk"));
}

#[test]
fn an_unreadable_feed_says_so_without_the_parser_text() {
    let json = serde_json::from_str::<u8>("nope").expect_err("not json");
    let text = describe(&Error::Serialization(json));
    assert!(
        text.starts_with("The update feed could not be read"),
        "{text}"
    );
    assert!(!text.contains("expected"), "{text}");
}

/// Anything not named above still gets a sentence with the plugin's own text
/// after it, rather than a blank row.
#[test]
fn an_unmapped_error_keeps_the_plugins_words_behind_a_plain_lead() {
    let text = describe(&Error::UnsupportedArch);
    assert!(text.starts_with("Update failed: "), "{text}");
    assert!(
        text.contains("Unsupported application architecture"),
        "{text}"
    );
}

/// No error sentence carries an em-dash: the row copy rule holds for the
/// backend's sentences too, since they are shown verbatim.
#[test]
fn no_sentence_carries_an_em_dash() {
    for error in [
        Error::ReleaseNotFound,
        Error::Network(String::new()),
        Error::EmptyEndpoints,
        Error::SignatureUtf8(String::new()),
        Error::TargetNotFound(String::new()),
        Error::Io(std::io::Error::other("x")),
        Error::UnsupportedOs,
    ] {
        let text = describe(&error);
        assert!(!text.contains('\u{2014}'), "{text}");
        assert!(!text.is_empty());
    }
}

#[test]
fn a_check_that_finds_nothing_reports_the_running_version_alone() {
    assert_eq!(
        outcome("0.2.0", None),
        UpdateCheck {
            current: "0.2.0".into(),
            available: None,
            notes: None,
        }
    );
}

#[test]
fn a_check_that_finds_a_release_carries_its_version_and_notes() {
    assert_eq!(
        outcome("0.2.0", Some(("0.3.1", Some("Fixes the keeper guard")))),
        UpdateCheck {
            current: "0.2.0".into(),
            available: Some("0.3.1".into()),
            notes: Some("Fixes the keeper guard".into()),
        }
    );
    assert_eq!(outcome("0.2.0", Some(("0.3.1", None))).notes, None);
}

/// The frontend reads these three keys by name.
#[test]
fn the_outcome_serialises_under_the_names_the_row_reads() {
    let json = serde_json::to_value(outcome("0.2.0", Some(("0.3.1", None)))).expect("json");
    assert_eq!(json["current"], "0.2.0");
    assert_eq!(json["available"], "0.3.1");
    assert!(json["notes"].is_null());
}
