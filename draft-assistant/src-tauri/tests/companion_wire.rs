//! Pairing, authentication and the read endpoints, over a real socket.

#[path = "companion/harness.rs"]
mod harness;
// Two more groups of tests over the same harness, in the same binary: a
// second test target would compile the harness again and warn about every
// helper that group does not happen to use.
#[path = "companion/chat_tests.rs"]
mod chat_tests;
#[path = "companion/ws_tests.rs"]
mod ws_tests;

use harness::host;
use serde_json::Value;

#[tokio::test]
async fn the_right_code_pairs_and_the_wrong_one_does_not() {
    let host = host("pair").await;
    let paired = host.pair_ok("Rob's iPhone", "phone").await;
    assert_eq!(paired.host_name, "Justin's Mac");
    assert!(!paired.token.is_empty());
    // The id the phone shows in the device list; never the token.
    assert!(!paired.device_id.is_empty());
    assert_ne!(paired.device_id, paired.token);

    let code = host.companion.hub.code();
    let wrong = if code == "000000" { "111111" } else { "000000" };
    let response = host
        .http
        .post(format!("{}/api/pair", host.base))
        .json(&serde_json::json!({ "code": wrong, "device_name": "Thief", "kind": "phone" }))
        .send()
        .await
        .expect("the request goes through");
    assert_eq!(response.status(), 403);
    let body: Value = response.json().await.expect("JSON");
    assert_eq!(body["error"], "wrong code");
}

#[tokio::test]
async fn five_wrong_codes_lock_the_sixth_attempt_out() {
    let host = host("lockout").await;
    let code = host.companion.hub.code();
    let wrong = if code == "000000" { "111111" } else { "000000" };
    for attempt in 0..5 {
        let response = host
            .http
            .post(format!("{}/api/pair", host.base))
            .json(&serde_json::json!({ "code": wrong, "device_name": "Thief", "kind": "phone" }))
            .send()
            .await
            .expect("the request goes through");
        assert_eq!(response.status(), 403, "attempt {attempt}");
    }
    // Even the right code, which is the point: guessing is not worth trying.
    let response = host
        .http
        .post(format!("{}/api/pair", host.base))
        .json(&serde_json::json!({ "code": code, "device_name": "Rob", "kind": "phone" }))
        .send()
        .await
        .expect("the request goes through");
    assert_eq!(response.status(), 429);
}

#[tokio::test]
async fn every_read_endpoint_refuses_a_request_with_no_token() {
    let host = host("unauthenticated").await;
    for path in [
        "/api/state",
        "/api/season",
        "/api/config",
        "/api/devices",
        "/api/headshot/q1",
        "/api/avatar/avatar-one",
        "/api/chat?screen=draft",
    ] {
        let (status, body) = host.get(path, "not-a-token").await;
        assert_eq!(status, 401, "{path}");
        assert_eq!(body["error"], "not paired", "{path}");
        // And with no header at all, not merely a bad one.
        let bare = host
            .http
            .get(format!("{}{path}", host.base))
            .send()
            .await
            .expect("the request goes through");
        assert_eq!(bare.status(), 401, "{path} with no header");
    }
}

#[tokio::test]
async fn the_page_itself_needs_no_token() {
    let host = host("page").await;
    for path in [
        "/",
        "/static/index.html",
        "/static/app.js",
        "/static/app.css",
    ] {
        let response = host
            .http
            .get(format!("{}{path}", host.base))
            .send()
            .await
            .expect("the request goes through");
        assert_eq!(response.status(), 200, "{path}");
    }
    let missing = host
        .http
        .get(format!("{}/static/config.json", host.base))
        .send()
        .await
        .expect("the request goes through");
    assert_eq!(missing.status(), 404);
}

#[tokio::test]
async fn state_and_season_are_the_views_the_desktop_validates() {
    let host = host("views").await;
    let paired = host.pair_ok("Rob's iPhone", "phone").await;
    let (status, view) = host.get("/api/state", &paired.token).await;
    assert_eq!(status, 200);
    assert_eq!(view["league"]["league_id"], "league-1");
    assert!(view["available"].is_array(), "the board came through");

    let (status, season) = host.get("/api/season", &paired.token).await;
    assert_eq!(status, 200);
    assert_eq!(season["league"]["league_id"], "league-1");
}

#[tokio::test]
async fn with_no_league_open_there_is_nothing_to_watch() {
    let host = host("no-league").await;
    let paired = host.pair_ok("Rob's iPhone", "phone").await;
    *host.state.loaded.lock().await = None;
    let (status, body) = host.get("/api/state", &paired.token).await;
    assert_eq!(status, 404);
    assert_eq!(body["error"], "no league loaded");
    let (status, _) = host.get("/api/season", &paired.token).await;
    assert_eq!(status, 404);
}

/// The one test that would have caught a key being published to the LAN.
#[tokio::test]
async fn the_config_endpoint_never_carries_a_secret() {
    let host = host("config-secrets").await;
    {
        // Everything secret the config can hold, set to something findable.
        let mut config = host.state.config.lock().await;
        config.anthropic_api_key = Some("sk-ant-should-never-leave-this-mac".to_string());
        config.chat_budget_usd = Some(12.5);
        config
            .chat_spend_usd
            .insert("draft.league-1".to_string(), 3.25);
    }
    let paired = host.pair_ok("Rob's iPhone", "phone").await;
    let response = host
        .http
        .get(format!("{}/api/config", host.base))
        .bearer_auth(&paired.token)
        .send()
        .await
        .expect("the request goes through");
    assert_eq!(response.status(), 200);
    // Asserted on the raw bytes, not on a parsed shape: a nested field nobody
    // thought about is exactly the way a key would get out.
    let raw = response.text().await.expect("a body");
    for secret in [
        "sk-ant",
        "api_key",
        "anthropic",
        "client_secret",
        "token",
        "chat_spend",
        "budget",
    ] {
        assert!(
            !raw.to_lowercase().contains(secret),
            "'{secret}' appears in /api/config: {raw}"
        );
    }
    let body: Value = serde_json::from_str(&raw).expect("JSON");
    assert_eq!(body["active_league_id"], "league-1");
    assert_eq!(body["my_user_id"], "u1");
    assert_eq!(body["host_name"], "Justin's Mac");
    assert_eq!(body["platform"], "sleeper");
    assert!(body["leagues"].is_array());
}

#[tokio::test]
async fn the_device_list_shows_what_is_paired() {
    let host = host("devices").await;
    let phone = host.pair_ok("Rob's iPhone", "phone").await;
    host.pair_ok("Kitchen Mac", "desktop").await;
    let (status, devices) = host.get("/api/devices", &phone.token).await;
    assert_eq!(status, 200);
    let devices = devices.as_array().expect("an array").clone();
    assert_eq!(devices.len(), 2);
    assert_eq!(devices[0]["name"], "Rob's iPhone");
    assert_eq!(devices[0]["kind"], "phone");
    assert_eq!(devices[1]["kind"], "desktop");
    // Nothing is connected until a socket is open, and no token is ever listed.
    assert_eq!(devices[0]["connected"], false);
    for device in &devices {
        assert!(device.get("token").is_none(), "{device}");
    }
}

#[tokio::test]
async fn a_picture_nobody_has_is_a_404_rather_than_an_error() {
    let host = host("headshot").await;
    let paired = host.pair_ok("Rob's iPhone", "phone").await;
    let response = host
        .http
        .get(format!("{}/api/headshot/nobody-at-all", host.base))
        .bearer_auth(&paired.token)
        .send()
        .await
        .expect("the request goes through");
    assert_eq!(response.status(), 404);
}

#[tokio::test]
async fn turning_the_server_off_closes_the_door() {
    let host = host("stop").await;
    let paired = host.pair_ok("Rob's iPhone", "phone").await;
    assert_eq!(host.get("/api/devices", &paired.token).await.0, 200);
    host.companion.stop();
    assert!(!host.companion.is_enabled());
    assert!(host.companion.url().is_none());
    let refused = host
        .http
        .get(format!("{}/api/devices", host.base))
        .bearer_auth(&paired.token)
        .timeout(std::time::Duration::from_secs(5))
        .send()
        .await;
    assert!(refused.is_err(), "the socket is still answering");
    // Devices survive the server being turned off — that is what Revoke is for.
    assert_eq!(host.companion.hub.devices().len(), 1);
}

#[tokio::test]
async fn the_url_on_screen_points_at_the_port_that_was_taken() {
    let host = host("url").await;
    let port = host.companion.port().expect("a port");
    let url = host.companion.url().expect("a URL");
    assert!(url.starts_with("http://"), "{url}");
    assert!(url.ends_with(&format!(":{port}/")), "{url}");
}
