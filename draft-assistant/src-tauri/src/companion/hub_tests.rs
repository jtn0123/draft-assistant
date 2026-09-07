//! Unit tests for the pairing, device and rate-limit bookkeeping. The wire
//! side of all of this is in `tests/companion_wire.rs`.

use super::*;
use serde::Serialize;
use std::net::IpAddr;

/// A directory of this test's own. The counter is what makes it its own: two
/// tests running in the same millisecond used to land on the same path, and
/// the second hub then started up holding the first one's paired devices.
fn scratch(label: &str) -> PathBuf {
    static NEXT: std::sync::atomic::AtomicU64 = std::sync::atomic::AtomicU64::new(0);
    let seq = NEXT.fetch_add(1, std::sync::atomic::Ordering::Relaxed);
    let dir = std::env::temp_dir().join(format!(
        "companion-hub-{label}-{}-{}-{seq}",
        std::process::id(),
        now_ms()
    ));
    std::fs::create_dir_all(&dir).expect("the scratch directory is creatable");
    dir
}

fn hub() -> CompanionHub {
    hub_in(scratch("hub"))
}

/// Always through the file store in the scratch directory. A test that used
/// `CompanionHub::new` would put a pairing code and a device token in the
/// developer's own login Keychain.
fn hub_in(dir: PathBuf) -> CompanionHub {
    let secrets = Box::new(crate::yahoo_secrets::FileStore::in_dir(dir.join("secrets")));
    CompanionHub::with_secrets("Test Mac".to_string(), dir, secrets).expect("the hub builds")
}

fn peer(last: u8) -> IpAddr {
    IpAddr::from([192, 168, 1, last])
}

fn attempt<'a>(code: &'a str, name: &'a str) -> PairAttempt<'a> {
    PairAttempt {
        code,
        name,
        kind: "phone",
        peer: peer(10),
        previous_device_id: None,
    }
}

fn paired(hub: &CompanionHub) -> (String, String) {
    paired_as(hub, "Phone", None)
}

/// Pair under a name, optionally saying which device this already was.
fn paired_as(hub: &CompanionHub, name: &str, previous: Option<&str>) -> (String, String) {
    let code = hub.code();
    let mut wanted = attempt(&code, name);
    wanted.previous_device_id = previous;
    match hub.pair(wanted).expect("pairing runs") {
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
        hub.pair(attempt(wrong, "Phone")).expect("runs"),
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
fn five_wrong_codes_lock_that_address_out_and_the_right_one_is_refused_too() {
    let hub = hub();
    let code = hub.code();
    let wrong = if code == "000000" { "111111" } else { "000000" };
    for _ in 0..5 {
        assert!(matches!(
            hub.pair(attempt(wrong, "Phone")).expect("runs"),
            PairOutcome::WrongCode
        ));
    }
    // The lockout is what makes the code worth only six digits: after this
    // the right code does not get in either until the minute is up.
    assert!(matches!(
        hub.pair(attempt(&code, "Phone")).expect("runs"),
        PairOutcome::LockedOut
    ));
}

/// The failure this prevents: the lockout only ever said so at debug, so a
/// phone that could not pair all evening left a log with nothing in it.
#[test]
fn an_address_being_locked_out_is_a_warning_in_the_log() {
    let capture = crate::applog::Capture::start();
    let hub = hub();
    let code = hub.code();
    let wrong = if code == "000000" { "111111" } else { "000000" };
    for _ in 0..5 {
        hub.pair(attempt(wrong, "Phone")).expect("runs");
    }
    assert!(
        !capture.saw("locked out"),
        "not before the lockout: {:?}",
        capture.lines()
    );
    hub.pair(attempt(&code, "Phone")).expect("runs");
    assert!(
        capture.saw("WARN companion: pairing refused, address locked out peer="),
        "{:?}",
        capture.lines()
    );
    assert!(
        !capture.lines().iter().any(|line| line.contains(&code)),
        "the code itself never reaches the log"
    );
}

#[test]
fn one_address_guessing_does_not_lock_the_rest_of_the_house_out() {
    let hub = hub();
    let code = hub.code();
    let wrong = if code == "000000" { "111111" } else { "000000" };
    for _ in 0..6 {
        let mut guess = attempt(wrong, "Thief");
        guess.peer = peer(66);
        hub.pair(guess).expect("runs");
    }
    // The guesser is locked out; the phone in the owner's hand is not.
    let mut locked = attempt(&code, "Thief");
    locked.peer = peer(66);
    assert!(matches!(
        hub.pair(locked).expect("runs"),
        PairOutcome::LockedOut
    ));
    let mut mine = attempt(&code, "Phone");
    mine.peer = peer(11);
    assert!(matches!(
        hub.pair(mine).expect("runs"),
        PairOutcome::Ok { .. }
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
    hub.set_port(Some(1));
    let mut rx = hub.subscribe();
    hub.publish_json("poll-health", serde_json::json!({ "ok": true }));
    let frame = rx.try_recv().expect("the frame was sent");
    let value: serde_json::Value = serde_json::from_str(&frame).expect("valid JSON");
    assert_eq!(value["type"], "poll-health");
    assert_eq!(value["payload"]["ok"], true);
}

#[test]
fn the_same_device_pairing_again_replaces_the_old_entry_and_its_token() {
    let hub = hub();
    let (first_token, first_id) = paired(&hub);
    let (second_token, second_id) = paired_as(&hub, "Phone", Some(&first_id));
    let devices = hub.devices();
    assert_eq!(devices.len(), 1, "{devices:?}");
    // The device keeps the id it already told the host it was.
    assert_eq!(second_id, first_id);
    assert!(hub.device_for(&second_token).is_some());
    assert!(
        hub.device_for(&first_token).is_none(),
        "the replaced device's token must stop working"
    );
}

#[test]
fn a_second_phone_with_the_same_name_is_numbered_rather_than_evicting_the_first() {
    let hub = hub();
    let (first_token, _) = paired_as(&hub, "iPhone", None);
    let (second_token, _) = paired_as(&hub, "iPhone", None);
    // Every iPhone calls itself "iPhone"; the second one must not throw the
    // first one off the host, which is what the old dedupe by name did.
    let names: Vec<String> = hub.devices().into_iter().map(|d| d.name).collect();
    assert_eq!(names, vec!["iPhone".to_string(), "iPhone 2".to_string()]);
    assert!(
        hub.device_for(&first_token).is_some(),
        "the first phone lost its token"
    );
    assert!(hub.device_for(&second_token).is_some());
    // And a device id nobody has seen before does not replace anything.
    paired_as(&hub, "iPhone", Some("not-a-device"));
    assert_eq!(hub.devices().len(), 3);
    assert_eq!(hub.devices()[2].name, "iPhone 3");
}

#[test]
fn a_used_code_is_spent_and_an_idle_one_is_replaced_after_ten_minutes() {
    let hub = hub();
    let before = hub.code();
    paired(&hub);
    assert_ne!(
        hub.code(),
        before,
        "the code that paired a phone was reused"
    );
    // A paired device does not freeze the code: the digits on a host that
    // has been up all afternoon are the ones somebody else has had longest
    // to read, and rotating them costs the paired phone nothing.
    assert!(hub.rotate_if_idle(now_ms() + CODE_MAX_AGE_MS * 2));

    let idle = hub_in(scratch("idle"));
    let first = idle.code();
    assert!(
        !idle.rotate_if_idle(now_ms()),
        "rotated a code minutes early"
    );
    assert!(idle.rotate_if_idle(now_ms() + CODE_MAX_AGE_MS + 1));
    assert_ne!(idle.code(), first);
}

#[test]
fn a_restarted_host_still_knows_the_phones_that_were_paired_with_it() {
    let dir = scratch("restart");
    let (token, device_id, code) = {
        let hub = hub_in(dir.clone());
        let (token, device_id) = paired(&hub);
        (token, device_id, hub.code())
    };
    // The whole of the failure this prevents: the host process goes away and
    // comes back, and the phone in the other room is still paired with it.
    let restarted = hub_in(dir);
    assert_eq!(
        restarted.code(),
        code,
        "the code on screen changed by itself"
    );
    let device = restarted
        .device_for(&token)
        .expect("the token survived the restart");
    assert_eq!(device.device_id, device_id);
    assert!(!device.connected, "nothing is connected to a fresh server");
    // And a revoke on the new process really does forget it.
    restarted.revoke().expect("revoke runs");
    assert!(restarted.device_for(&token).is_none());
}

#[test]
fn a_name_that_is_taken_gets_a_number_and_one_that_is_free_does_not() {
    assert_eq!(unique_name("iPhone", &[]), "iPhone");
    assert_eq!(unique_name("iPhone", &["iPhone"]), "iPhone 2");
    assert_eq!(unique_name("iPhone", &["iPhone", "iPhone 2"]), "iPhone 3");
    assert_eq!(unique_name("iPad", &["iPhone"]), "iPad");
}

#[test]
fn rotating_the_code_never_costs_a_paired_phone_its_token() {
    let hub = hub();
    let (token, device_id) = paired(&hub);
    assert!(hub.rotate_if_idle(now_ms() + CODE_MAX_AGE_MS + 1));
    // The whole point of rotating: what a *new* device would have to type
    // changes, and what an already paired one holds does not.
    assert!(hub.device_for(&token).is_some());
    assert!(hub.still_paired(&device_id));
}

#[test]
fn the_token_a_re_pair_replaced_is_announced_so_its_socket_can_close() {
    let hub = hub();
    let mut closes = hub.subscribe_closes();
    let (first_token, first_id) = paired(&hub);
    assert!(closes.try_recv().is_err(), "nothing was replaced yet");
    let (second_token, _) = paired_as(&hub, "Phone", Some(&first_id));
    // The failure this prevents: the device id is unchanged, so nothing else
    // tells the socket opened on the old token that the host has replaced it.
    assert_eq!(closes.try_recv().ok(), Some(Closing::Token(first_token)));
    assert!(hub.device_for(&second_token).is_some());
}

/// A payload that records whether anything ever asked it to serialise, so a
/// test can tell a skipped publish from one that merely went nowhere.
struct CountedPayload {
    serialised: Arc<std::sync::atomic::AtomicUsize>,
}

impl Serialize for CountedPayload {
    fn serialize<S: serde::Serializer>(&self, serializer: S) -> Result<S::Ok, S::Error> {
        self.serialised
            .fetch_add(1, std::sync::atomic::Ordering::SeqCst);
        serializer.serialize_str("payload")
    }
}

#[test]
fn publishing_with_nobody_listening_does_not_serialise_the_payload() {
    let hub = hub();
    hub.set_port(Some(1));
    let serialised = Arc::new(std::sync::atomic::AtomicUsize::new(0));
    let payload = CountedPayload {
        serialised: serialised.clone(),
    };
    // The failure this prevents: every three second poll tick turned a whole
    // draft view into tens of kilobytes of JSON and handed it to a broadcast
    // channel with no receivers, which threw it straight away.
    hub.publish("draft-updated", &payload);
    assert_eq!(serialised.load(std::sync::atomic::Ordering::SeqCst), 0);

    let mut rx = hub.subscribe();
    hub.publish("draft-updated", &payload);
    assert_eq!(serialised.load(std::sync::atomic::Ordering::SeqCst), 1);
    let frame = rx.try_recv().expect("the subscriber got the frame");
    let value: serde_json::Value = serde_json::from_str(&frame).expect("valid JSON");
    assert_eq!(value["type"], "draft-updated");
    assert_eq!(value["payload"], "payload");
}

#[test]
fn publish_devices_still_reaches_the_webview_with_no_subscriber() {
    let hub = hub();
    let seen: Arc<Mutex<Vec<String>>> = Arc::new(Mutex::new(Vec::new()));
    let recorded = seen.clone();
    hub.set_emit(Arc::new(move |kind: &str, _payload: serde_json::Value| {
        recorded
            .lock()
            .unwrap_or_else(|e| e.into_inner())
            .push(kind.to_string());
    }));
    assert!(!hub.has_listeners());
    // The failure this prevents: skipping the broadcast when nobody is
    // listening must not take the host's own settings screen with it, since
    // the first device pairs before any socket exists.
    paired(&hub);
    let kinds = seen.lock().unwrap_or_else(|e| e.into_inner()).clone();
    assert!(kinds.iter().any(|k| k == "companion-devices"), "{kinds:?}");
}

#[test]
fn a_server_that_is_off_publishes_nothing_even_to_a_socket_still_closing() {
    let hub = hub();
    let mut rx = hub.subscribe();
    // The failure this prevents: the toggle went off, the sockets were still
    // draining, and the publish path only asked whether anyone was listening.
    // They were, so the next poll tick still reached every phone.
    hub.publish_json("draft-updated", serde_json::json!({ "picks": 3 }));
    assert!(
        rx.try_recv().is_err(),
        "a frame went out with the server off"
    );
    hub.set_port(Some(1));
    hub.publish_json("draft-updated", serde_json::json!({ "picks": 4 }));
    assert!(rx.try_recv().is_ok());
    hub.set_port(None);
    hub.publish("draft-updated", &serde_json::json!({ "picks": 5 }));
    assert!(
        rx.try_recv().is_err(),
        "a frame went out after the server stopped"
    );
}

#[test]
fn stopping_tells_every_socket_to_go_without_calling_it_revoked() {
    let hub = hub();
    let mut closes = hub.subscribe_closes();
    hub.close_everyone();
    assert_eq!(closes.try_recv().expect("a close"), Closing::Everyone);
}
