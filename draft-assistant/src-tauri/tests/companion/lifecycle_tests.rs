//! Turning the companion on and off, and what keeps running while it is up.

use crate::harness::{self, host, wait_until};

/// The whole failure: the Mac crashes mid-draft, comes back, and every phone
/// in the room stays dark until somebody walks over and opens Settings.
#[tokio::test]
async fn a_server_the_user_left_on_comes_back_up_by_itself() {
    use draft_assistant_lib::commands_companion::{autostart, autostart_port};
    use draft_assistant_lib::companion::CompanionServer;

    let data_dir = harness::scratch_dir("autostart");
    let state = std::sync::Arc::new(harness::fixture_state(&data_dir));
    let companion = std::sync::Arc::new(
        CompanionServer::sandboxed("Justin's Mac".to_string(), data_dir)
            .expect("the companion builds"),
    );
    companion.attach(
        state,
        std::sync::Arc::new(|_: &str, _: serde_json::Value| {}),
    );

    let mut config = draft_assistant_lib::engine::AppConfig::default();
    // Off is off: a launch must not open a port nobody asked for. With the
    // port `None` nothing is spawned at all, so there is nothing to wait for
    // before looking.
    assert_eq!(autostart_port(&config), None);
    autostart(&companion, autostart_port(&config));
    assert!(!companion.is_enabled());

    config.companion_enabled = true;
    // Port 0 so the kernel picks one and the test never collides.
    config.companion_port = Some(0);
    autostart(&companion, autostart_port(&config));
    wait_until("the phone connection is on", || companion.is_enabled()).await;
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
    wait_until("the code has rotated", || {
        host.companion.hub.code() != before
    })
    .await;
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

/// Also the failure Companion 7 names: `companion_status` used to shell out
/// to the Tailscale CLI on every devices event to answer `tailscale_url`.
/// Now the URL on screen is whatever this refresh last read.
#[tokio::test]
async fn origins_and_the_tailnet_url_follow_the_machine_onto_a_tailnet() {
    use draft_assistant_lib::companion::net::Reach;
    use draft_assistant_lib::companion::server::spawn_origin_refresh;
    use std::sync::atomic::{AtomicBool, AtomicU32, Ordering};

    let host = host("refresh").await;
    let before = host.companion.hub.origins();
    let url_before = host.companion.tailscale_url();
    assert!(!before.iter().any(|o| o.contains("100.101.102.103")));
    // Tailscale comes up after the server did: the machine grows an address.
    let on_tailnet = std::sync::Arc::new(AtomicBool::new(false));
    let seen = on_tailnet.clone();
    // How many times the refresh has looked, so the "nothing changed" half
    // below waits on looks having happened rather than on a clock.
    let looks = std::sync::Arc::new(AtomicU32::new(0));
    let counted = looks.clone();
    let same = before.clone();
    let same_url = url_before.clone();
    let task = spawn_origin_refresh(
        host.companion.hub.clone(),
        std::time::Duration::from_millis(10),
        move |port| {
            counted.fetch_add(1, Ordering::SeqCst);
            let mut reach = Reach {
                origins: same.clone(),
                tailscale_url: same_url.clone(),
            };
            if seen.load(Ordering::SeqCst) {
                reach.origins.push(format!("http://100.101.102.103:{port}"));
                reach.tailscale_url = Some(format!("http://100.101.102.103:{port}/"));
            }
            reach
        },
    );
    wait_until("the refresh has looked a few times", || {
        looks.load(Ordering::SeqCst) >= 3
    })
    .await;
    // Nothing changed, so nothing was written.
    assert_eq!(host.companion.hub.origins(), before);
    assert_eq!(host.companion.tailscale_url(), url_before);
    on_tailnet.store(true, Ordering::SeqCst);
    let port = host.companion.port().expect("running");
    let want = format!("http://100.101.102.103:{port}");
    wait_until("the tailnet address is in the origins", || {
        host.companion.hub.origins().contains(&want)
    })
    .await;
    // The status the Settings panel shows reads the same cache; no CLI runs.
    assert_eq!(
        host.companion.tailscale_url().as_deref(),
        Some(format!("http://100.101.102.103:{port}/").as_str())
    );
    // And the page a phone loads now names the tailnet socket, so the
    // WebSocket is not refused by the page's own policy.
    let page = host
        .http
        .get(format!("{}/", host.base))
        .send()
        .await
        .expect("page");
    let csp = page
        .headers()
        .get("content-security-policy")
        .expect("csp")
        .to_str()
        .expect("ascii")
        .to_string();
    assert!(
        csp.contains(&format!("ws://100.101.102.103:{port}")),
        "{csp}"
    );
    host.companion.stop();
    tokio::time::timeout(std::time::Duration::from_secs(5), task)
        .await
        .expect("the refresh task stops with the server")
        .expect("the refresh task did not panic");
}

/// The failure this prevents: the headless `companion_host` and the desktop
/// app on one Mac kept their pairings under one account, so pairing a phone
/// against the host replaced every phone paired to the desktop. Each server
/// is now told which account is its own when it is built, and a restart under
/// the other account sees nothing of the first.
#[tokio::test]
async fn a_server_built_under_the_headless_account_keeps_its_pairings_apart() {
    use draft_assistant_lib::companion::pairing::PairAttempt;
    use draft_assistant_lib::companion::CompanionServer;
    use draft_assistant_lib::yahoo_secrets::Item;

    let data_dir = harness::scratch_dir("headless-account");
    let build = |item: Item| {
        let state = std::sync::Arc::new(harness::fixture_state(&data_dir));
        let companion = std::sync::Arc::new(
            CompanionServer::sandboxed_under("Justin's Mac".to_string(), data_dir.clone(), item)
                .expect("the companion builds"),
        );
        companion.attach(
            state,
            std::sync::Arc::new(|_: &str, _: serde_json::Value| {}),
        );
        companion
    };

    let headless = build(Item::CompanionDevicesHeadless);
    let code = headless.hub.code();
    headless
        .hub
        .pair(PairAttempt {
            code: &code,
            name: "Rob's iPhone",
            kind: "phone",
            peer: std::net::IpAddr::from([192, 168, 1, 10]),
            previous_device_id: None,
        })
        .expect("pairs");
    assert_eq!(headless.hub.devices().len(), 1);
    // A used code is spent, so what a restart has to bring back is the one
    // that replaced it.
    let code = headless.hub.code();
    drop(headless);

    // The desktop app, over the same secret store: no phone, and its own code.
    let desktop = build(Item::CompanionDevices);
    assert!(
        desktop.hub.devices().is_empty(),
        "the desktop read the headless host's pairings"
    );
    assert_ne!(desktop.hub.code(), code);
    drop(desktop);

    // The headless host again: the phone is still paired.
    let again = build(Item::CompanionDevicesHeadless);
    assert_eq!(again.hub.devices().len(), 1);
    assert_eq!(again.hub.devices()[0].name, "Rob's iPhone");
    assert_eq!(again.hub.code(), code);
}
