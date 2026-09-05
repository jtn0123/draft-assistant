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
        "/static/helpers.js",
        "/static/clock.js",
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

/// A follower desktop's webview and the Vite dev server are other origins;
/// without these headers the browser drops every answer and the preflight
/// would be a 405.
#[tokio::test]
async fn another_origin_is_allowed_to_call_the_api() {
    let host = harness::host("cors").await;
    let preflight = host
        .http
        .request(reqwest::Method::OPTIONS, format!("{}/api/pair", host.base))
        .header("origin", "tauri://localhost")
        .header("access-control-request-method", "POST")
        .send()
        .await
        .expect("preflight goes through");
    assert_eq!(preflight.status(), 204);
    let allow = |name: &str| {
        preflight
            .headers()
            .get(name)
            .map(|v| v.to_str().unwrap_or("").to_string())
            .unwrap_or_default()
    };
    assert_eq!(allow("access-control-allow-origin"), "*");
    assert!(allow("access-control-allow-headers").contains("authorization"));
    assert!(allow("access-control-allow-methods").contains("POST"));

    let paired = host.pair_ok("Rob's iPhone", "phone").await;
    let response = host
        .http
        .get(format!("{}/api/state", host.base))
        .bearer_auth(&paired.token)
        .header("origin", "http://localhost:1420")
        .send()
        .await
        .expect("the request goes through");
    assert_eq!(response.status(), 200);
    assert_eq!(
        response
            .headers()
            .get("access-control-allow-origin")
            .map(|v| v.to_str().unwrap_or("")),
        Some("*")
    );
}

/// Every response the phone loads the page from carries the policy, so a
/// string that somehow became markup has no script to run and nowhere to send
/// what it found.
#[tokio::test]
async fn the_page_is_served_under_a_content_security_policy() {
    let host = host("csp").await;
    for path in ["/", "/static/app.js", "/static/app.css", "/static/clock.js"] {
        let response = host
            .http
            .get(format!("{}{path}", host.base))
            .send()
            .await
            .expect("the request goes through");
        let header = |name: &str| {
            response
                .headers()
                .get(name)
                .map(|v| v.to_str().unwrap_or("").to_string())
                .unwrap_or_default()
        };
        let csp = header("content-security-policy");
        assert!(csp.contains("default-src 'none'"), "{path}: {csp}");
        assert!(csp.contains("script-src 'self'"), "{path}: {csp}");
        assert!(csp.contains("img-src 'self' data:"), "{path}: {csp}");
        assert!(csp.contains("connect-src 'self' ws: wss:"), "{path}: {csp}");
        assert!(csp.contains("base-uri 'none'"), "{path}: {csp}");
        assert!(csp.contains("form-action 'none'"), "{path}: {csp}");
        assert_eq!(header("x-content-type-options"), "nosniff", "{path}");
        assert_eq!(header("referrer-policy"), "no-referrer", "{path}");
    }
}

/// The failure this prevents: a page the phone has open in another tab
/// posting to the host in the background, on a token the browser will happily
/// attach to a request the user never made.
#[tokio::test]
async fn a_page_on_another_site_cannot_post_to_the_host() {
    let host = host("origin").await;
    let paired = host.pair_ok("Rob's iPhone", "phone").await;
    let post = |origin: &'static str| {
        let http = host.http.clone();
        let base = host.base.clone();
        let token = paired.token.clone();
        async move {
            http.post(format!("{base}/api/chat"))
                .bearer_auth(token)
                .header("origin", origin)
                .json(&serde_json::json!({ "screen": "draft", "text": "hello?" }))
                .send()
                .await
                .expect("the request goes through")
                .status()
                .as_u16()
        }
    };
    assert_eq!(post("https://evil.example.com").await, 403);
    // The follower desktop and the dev server are the two other origins that
    // are ours, and they must go on working.
    assert_ne!(post("tauri://localhost").await, 403);
    assert_ne!(post("http://localhost:1420").await, 403);
    // The phone page itself: its own origin is this server.
    let port = host.companion.port().expect("a port");
    let same = host
        .http
        .post(format!("{}/api/chat", host.base))
        .bearer_auth(&paired.token)
        .header("origin", format!("http://127.0.0.1:{port}"))
        .json(&serde_json::json!({ "screen": "draft", "text": "hello?" }))
        .send()
        .await
        .expect("the request goes through");
    assert_ne!(same.status().as_u16(), 403);
    // A read is not a change, and stays open to anyone holding the token.
    let read = host
        .http
        .get(format!("{}/api/devices", host.base))
        .bearer_auth(&paired.token)
        .header("origin", "https://evil.example.com")
        .send()
        .await
        .expect("the request goes through");
    assert_eq!(read.status(), 200);
}

/// The whole failure: the host process restarts and every phone in the house
/// is silently unpaired, with no way to know but the next request failing.
#[tokio::test]
async fn a_phone_stays_paired_across_a_restart_of_the_host() {
    let first = host("restart").await;
    let paired = first.pair_ok("Rob's iPhone", "phone").await;
    assert_eq!(first.get("/api/state", &paired.token).await.0, 200);
    first.companion.stop();

    // The same data directory and the same league: a new process, nothing
    // else. The token the phone is holding still opens the door.
    let restarted = harness::host_over(first.data_dir.clone(), first.state.clone()).await;
    let (status, view) = restarted.get("/api/state", &paired.token).await;
    assert_eq!(status, 200, "the phone was silently unpaired: {view}");
    let devices = restarted.companion.hub.devices();
    assert_eq!(devices.len(), 1);
    assert_eq!(devices[0].name, "Rob's iPhone");
    assert!(
        !devices[0].connected,
        "nothing is connected to a fresh server"
    );
}

#[tokio::test]
async fn two_phones_with_the_same_name_both_stay_paired() {
    let host = host("two-phones").await;
    let first = host.pair_ok("iPhone", "phone").await;
    let second = host.pair_ok("iPhone", "phone").await;
    // Both work: the second phone in a house used to evict the first, whose
    // owner then found the app asking for a code again for no visible reason.
    assert_eq!(host.get("/api/devices", &first.token).await.0, 200);
    assert_eq!(host.get("/api/devices", &second.token).await.0, 200);
    let names: Vec<String> = host
        .companion
        .hub
        .devices()
        .into_iter()
        .map(|d| d.name)
        .collect();
    assert_eq!(names, vec!["iPhone".to_string(), "iPhone 2".to_string()]);

    // The first phone pairing again, saying which device it is, replaces
    // itself rather than becoming a third entry.
    let again = host
        .pair_again("iPhone", "phone", Some(&first.device_id))
        .await;
    assert_eq!(again.device_id, first.device_id);
    assert_eq!(host.companion.hub.devices().len(), 2);
    let (status, _) = host.get("/api/devices", &first.token).await;
    assert_eq!(status, 401, "the replaced token still works");
}

/// A code that has paired a phone is spent: the next device types a new one.
#[tokio::test]
async fn the_code_on_screen_changes_after_it_has_been_used() {
    let host = host("code-rotation").await;
    let used = host.companion.hub.code();
    host.pair_ok("Rob's iPhone", "phone").await;
    assert_ne!(host.companion.hub.code(), used, "the code was reused");
    let response = host
        .http
        .post(format!("{}/api/pair", host.base))
        .json(&serde_json::json!({ "code": used, "device_name": "Thief", "kind": "phone" }))
        .send()
        .await
        .expect("the request goes through");
    assert_eq!(response.status(), 403);
}

/// The per-address pairing lockout itself is unit-tested in `hub_tests`; what
/// this file proves is that the peer address reaches it at all, since every
/// pairing test here would fail with a 500 if the connect info were missing.
///
/// A code left on screen all afternoon is replaced, and the old one is no
/// longer worth anything to whoever glanced at it.
#[tokio::test]
async fn an_idle_code_is_replaced_after_ten_minutes() {
    let host = host("idle-code").await;
    let before = host.companion.hub.code();
    let later = draft_assistant_lib::companion::hub::now_ms()
        + draft_assistant_lib::companion::hub::CODE_MAX_AGE_MS
        + 1;
    assert!(host.companion.hub.rotate_if_idle(later));
    assert_ne!(host.companion.hub.code(), before);
    let stale = host
        .http
        .post(format!("{}/api/pair", host.base))
        .json(&serde_json::json!({ "code": before, "device_name": "Thief", "kind": "phone" }))
        .send()
        .await
        .expect("the request goes through");
    assert_eq!(stale.status(), 403);
    // The host's own screen hears about it the way it hears about devices.
    assert!(host
        .emitted_kinds()
        .contains(&"companion-devices".to_string()));
}
