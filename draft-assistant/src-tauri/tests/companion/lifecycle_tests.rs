//! Turning the companion on and off, and what keeps running while it is up.

use crate::harness::{self, host};

/// The whole failure: the Mac crashes mid-draft, comes back, and every phone
/// in the room stays dark until somebody walks over and opens Settings.
#[tokio::test]
async fn a_server_the_user_left_on_comes_back_up_by_itself() {
    use draft_assistant_lib::commands_companion::{autostart, autostart_port};
    use draft_assistant_lib::companion::CompanionServer;

    let data_dir = harness::scratch_dir("autostart");
    let state = std::sync::Arc::new(harness::fixture_state(&data_dir));
    let companion = std::sync::Arc::new(
        CompanionServer::new("Justin's Mac".to_string(), data_dir).expect("the companion builds"),
    );
    companion.attach(
        state,
        std::sync::Arc::new(|_: &str, _: serde_json::Value| {}),
    );

    let mut config = draft_assistant_lib::engine::AppConfig::default();
    // Off is off: a launch must not open a port nobody asked for.
    autostart(&companion, autostart_port(&config));
    tokio::time::sleep(std::time::Duration::from_millis(50)).await;
    assert!(!companion.is_enabled());

    config.companion_enabled = true;
    // Port 0 so the kernel picks one and the test never collides.
    config.companion_port = Some(0);
    autostart(&companion, autostart_port(&config));
    for _ in 0..100 {
        if companion.is_enabled() {
            break;
        }
        tokio::time::sleep(std::time::Duration::from_millis(20)).await;
    }
    assert!(companion.is_enabled(), "the phone connection stayed off");
    assert!(companion.url().is_some());
    companion.stop();
}

/// The failure this prevents: `rotate_if_idle` only ever ran when somebody
/// asked for the code, so a host with the Settings panel closed left the same
/// six digits on screen for the whole afternoon.
#[tokio::test]
async fn an_idle_code_rotates_with_nobody_looking_at_it() {
    use draft_assistant_lib::companion::hub::CODE_MAX_AGE_MS;
    use draft_assistant_lib::companion::server::spawn_rotation;

    let host = host("rotate").await;
    // A device is paired, because a paired host is exactly the one that used
    // to sit on one code all draft.
    host.pair_ok("Rob's iPhone", "phone").await;
    let before = host.companion.hub.code();
    // An injected clock, ten minutes on, rather than ten minutes of waiting.
    let task = spawn_rotation(
        host.companion.hub.clone(),
        std::time::Duration::from_millis(10),
        || draft_assistant_lib::companion::hub::now_ms() + CODE_MAX_AGE_MS + 1,
    );
    for _ in 0..100 {
        if host.companion.hub.code() != before {
            break;
        }
        tokio::time::sleep(std::time::Duration::from_millis(20)).await;
    }
    assert_ne!(host.companion.hub.code(), before, "the code never rotated");
    // The host's own panel is told, so the digits on screen are the live ones.
    assert!(host
        .emitted_kinds()
        .contains(&"companion-devices".to_string()));
    // And the task ends with the server rather than ticking for ever.
    host.companion.stop();
    tokio::time::timeout(std::time::Duration::from_secs(5), task)
        .await
        .expect("the rotation task stops with the server")
        .expect("the rotation task did not panic");
}
