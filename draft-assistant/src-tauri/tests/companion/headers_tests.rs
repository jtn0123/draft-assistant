//! The security headers the page is served under, and who may post here.

use crate::harness::{host, Host, Paired};

/// Every response the phone loads the page from carries the policy, so a
/// string that somehow became markup has no script to run and nowhere to send
/// what it found.
#[tokio::test]
async fn the_page_is_served_under_a_content_security_policy() {
    let host = host("csp").await;
    let port = host.companion.port().expect("a port");
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
        // The socket origin is this server's own, spelled out. A bare `ws:`
        // scheme here let the page open a socket to any host on the network.
        assert!(
            csp.contains(&format!("ws://127.0.0.1:{port}")),
            "{path}: {csp}"
        );
        assert!(!csp.contains(" ws:;"), "{path}: {csp}");
        assert!(!csp.contains(" ws: wss:"), "{path}: {csp}");
        assert!(csp.contains("frame-ancestors 'none'"), "{path}: {csp}");
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
    // The failure this prevents: another machine on the same Wi-Fi is a
    // private address too, and was waved through for exactly that reason.
    let port = host.companion.port().expect("a port");
    assert_eq!(
        post_from(&host, &paired, format!("http://192.168.1.99:{port}")).await,
        403
    );
    assert_eq!(
        post_from(&host, &paired, format!("http://10.11.12.13:{port}")).await,
        403
    );
    // The follower desktop and the dev server are the two other origins that
    // are ours, and they must go on working.
    assert_ne!(post("tauri://localhost").await, 403);
    assert_ne!(post("http://localhost:1420").await, 403);
    // The phone page itself: its own origin is this server.
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

/// One POST from a named origin, as a status.
async fn post_from(host: &Host, paired: &Paired, origin: String) -> u16 {
    host.http
        .post(format!("{}/api/chat", host.base))
        .bearer_auth(&paired.token)
        .header("origin", origin)
        .json(&serde_json::json!({ "screen": "draft", "text": "hello?" }))
        .send()
        .await
        .expect("the request goes through")
        .status()
        .as_u16()
}
