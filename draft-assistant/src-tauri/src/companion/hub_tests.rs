//! Unit tests for the pairing, device and rate-limit bookkeeping. The wire
//! side of all of this is in `tests/companion_wire.rs`.

use super::*;

fn hub() -> CompanionHub {
    CompanionHub::new("Test Mac".to_string()).expect("the hub builds")
}

fn paired(hub: &CompanionHub) -> (String, String) {
    let code = hub.code();
    match hub.pair(&code, "Phone", "phone").expect("pairing runs") {
        PairOutcome::Ok {
            token, device_id, ..
        } => (token, device_id),
        _ => panic!("the right code did not pair"),
    }
}

#[test]
fn the_right_code_pairs_and_the_wrong_one_does_not() {
    let hub = hub();
    let code = hub.code();
    let wrong = if code == "000000" { "111111" } else { "000000" };
    assert!(matches!(
        hub.pair(wrong, "Phone", "phone").expect("runs"),
        PairOutcome::WrongCode
    ));
    let (token, device_id) = paired(&hub);
    let device = hub.device_for(&token).expect("the token is known");
    assert_eq!(device.device_id, device_id);
    assert_eq!(device.kind, "phone");
    assert!(!device.connected);
    assert!(hub.device_for("not-a-token").is_none());
}

#[test]
fn five_wrong_codes_lock_pairing_out_and_the_right_one_is_refused_too() {
    let hub = hub();
    let code = hub.code();
    let wrong = if code == "000000" { "111111" } else { "000000" };
    for _ in 0..5 {
        assert!(matches!(
            hub.pair(wrong, "Phone", "phone").expect("runs"),
            PairOutcome::WrongCode
        ));
    }
    // The lockout is what makes the code worth only six digits: after this
    // the right code does not get in either until the minute is up.
    assert!(matches!(
        hub.pair(&code, "Phone", "phone").expect("runs"),
        PairOutcome::LockedOut
    ));
}

#[test]
fn revoking_rotates_the_code_and_drops_every_device() {
    let hub = hub();
    let before = hub.code();
    let (token, device_id) = paired(&hub);
    assert!(hub.still_paired(&device_id));
    let after = hub.revoke().expect("revoke runs");
    assert_ne!(before, after);
    assert_eq!(hub.code(), after);
    assert!(hub.device_for(&token).is_none());
    assert!(!hub.still_paired(&device_id));
    assert!(hub.devices().is_empty());
}

#[test]
fn a_device_counts_as_connected_only_while_it_holds_a_socket() {
    let hub = hub();
    let (_, device_id) = paired(&hub);
    hub.socket_changed(&device_id, true);
    hub.socket_changed(&device_id, true);
    assert!(hub.devices()[0].connected);
    hub.socket_changed(&device_id, false);
    assert!(hub.devices()[0].connected, "one socket is still open");
    hub.socket_changed(&device_id, false);
    assert!(!hub.devices()[0].connected);
    // Never underflows, however many closes arrive.
    hub.socket_changed(&device_id, false);
    assert!(!hub.devices()[0].connected);
}

#[test]
fn a_device_gets_ten_questions_a_minute_and_no_more() {
    let hub = hub();
    let (_, device_id) = paired(&hub);
    for n in 0..10 {
        assert!(hub.allow_chat_post(&device_id), "question {n} was refused");
    }
    assert!(!hub.allow_chat_post(&device_id));
    // An unknown device is refused rather than given a fresh allowance.
    assert!(!hub.allow_chat_post("nobody"));
}

#[test]
fn a_device_name_is_trimmed_bounded_and_never_blank() {
    assert_eq!(display_name("  Rob's iPhone "), "Rob's iPhone");
    assert_eq!(display_name("   "), "A device");
    assert_eq!(display_name(&"x".repeat(200)).chars().count(), 60);
}

#[test]
fn events_reach_a_subscriber_as_type_and_payload() {
    let hub = hub();
    let mut rx = hub.subscribe();
    hub.publish_json("poll-health", serde_json::json!({ "ok": true }));
    let frame = rx.try_recv().expect("the frame was sent");
    let value: serde_json::Value = serde_json::from_str(&frame).expect("valid JSON");
    assert_eq!(value["type"], "poll-health");
    assert_eq!(value["payload"]["ok"], true);
}

#[test]
fn pairing_again_under_the_same_name_replaces_the_old_entry_and_its_token() {
    let hub = hub();
    let (first_token, _) = paired(&hub);
    let (second_token, _) = paired(&hub);
    let devices = hub.devices();
    assert_eq!(devices.len(), 1, "{devices:?}");
    assert!(hub.device_for(&second_token).is_some());
    assert!(
        hub.device_for(&first_token).is_none(),
        "the replaced device's token must stop working"
    );
}
