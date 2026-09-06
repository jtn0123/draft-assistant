//! The HTTPS listener beside the plain one, over a certificate the test
//! minted itself. `tailscale cert` is never run: the certificate is
//! self-signed for a made-up MagicDNS name, which is why the client below
//! is told not to check it.

use crate::harness::{fixture_state, host, host_over_tls, scratch_dir, Host};
use draft_assistant_lib::companion::tls::TlsSource;
use std::sync::Arc;

const NAME: &str = "justins-mac.tail1234.ts.net";

/// A host serving HTTPS from a fresh self-signed certificate for [`NAME`].
async fn secure_host(label: &str) -> Host {
    let data_dir = scratch_dir(label);
    let mut params = rcgen::CertificateParams::new(vec![NAME.to_string()]).expect("params");
    params.not_after = rcgen::date_time_ymd(2035, 1, 1);
    let key = rcgen::KeyPair::generate().expect("a key pair");
    let cert = params.self_signed(&key).expect("a certificate");
    let cert_path = data_dir.join("test.crt");
    let key_path = data_dir.join("test.key");
    std::fs::write(&cert_path, cert.pem()).expect("cert written");
    std::fs::write(&key_path, key.serialize_pem()).expect("key written");
    let state = Arc::new(fixture_state(&data_dir));
    host_over_tls(
        data_dir,
        state,
        Some(TlsSource::Files {
            host: NAME.to_string(),
            cert: cert_path,
            key: key_path,
        }),
    )
    .await
}

/// A client that accepts the self-signed test certificate and nothing about
/// the name it is connecting to, since 127.0.0.1 is not the tailnet name.
fn trusting_client() -> reqwest::Client {
    reqwest::Client::builder()
        .danger_accept_invalid_certs(true)
        .danger_accept_invalid_hostnames(true)
        .build()
        .expect("a client")
}

#[tokio::test]
async fn https_origin_is_accepted_once_the_cert_listener_is_up() {
    let host = secure_host("https").await;
    let http_port = host.companion.port().expect("http port");
    let https_port = host
        .companion
        .https_port()
        .expect("the https listener is up");
    assert_ne!(https_port, http_port);
    let origin = format!("https://{NAME}:{https_port}");
    // The URL on screen is the secure one: it is the one with the padlock,
    // the wake lock and Add to Home Screen.
    assert_eq!(
        host.companion.tailscale_url().as_deref(),
        Some(format!("{origin}/").as_str())
    );
    assert!(host.companion.hub.origins().contains(&origin));

    // The same page, over TLS, with a policy that names the secure socket.
    let client = trusting_client();
    let page = client
        .get(format!("https://127.0.0.1:{https_port}/"))
        .send()
        .await
        .expect("the https request goes through");
    assert_eq!(page.status(), 200);
    let csp = page
        .headers()
        .get("content-security-policy")
        .and_then(|v| v.to_str().ok())
        .unwrap_or("")
        .to_string();
    assert!(csp.contains(&format!("wss://{NAME}:{https_port}")), "{csp}");
    assert!(page
        .text()
        .await
        .expect("a body")
        .contains("companion-root"));

    // The failure this prevents: the page loaded over https, and its own
    // pairing POST was refused as coming from another site.
    let code = host.companion.hub.code();
    let paired = client
        .post(format!("https://127.0.0.1:{https_port}/api/pair"))
        .header("origin", &origin)
        .json(&serde_json::json!({ "code": code, "device_name": "Phone", "kind": "phone" }))
        .send()
        .await
        .expect("the pair request goes through");
    assert_eq!(paired.status(), 200);

    // Off takes both listeners down.
    host.companion.stop();
    tokio::time::sleep(std::time::Duration::from_millis(50)).await;
    assert_eq!(host.companion.https_port(), None);
    assert!(
        std::net::TcpStream::connect(("127.0.0.1", https_port)).is_err(),
        "the https port is still answering after stop"
    );
}

#[tokio::test]
async fn no_cert_means_http_only_and_the_same_origins_as_before() {
    let host = host("http-only").await;
    assert_eq!(host.companion.https_port(), None);
    let origins = host.companion.hub.origins();
    assert!(!origins.is_empty());
    assert!(
        origins.iter().all(|o| o.starts_with("http://")),
        "{origins:?}"
    );
    let port = host.companion.port().expect("a port");
    // The plain listener is exactly the companion of before.
    let page = host
        .http
        .get(format!("http://127.0.0.1:{port}/"))
        .send()
        .await
        .expect("the request goes through");
    assert_eq!(page.status(), 200);
    assert_eq!(
        host.companion.url(),
        Some(format!(
            "http://{}:{port}/",
            draft_assistant_lib::companion::net::lan_ip()
        ))
    );
}

#[tokio::test]
async fn the_manifest_and_service_worker_are_served_for_installing_the_page() {
    let host = host("pwa").await;
    let get = |path: &str| host.http.get(format!("{}{path}", host.base)).send();
    let manifest = get("/static/manifest.webmanifest").await.expect("manifest");
    assert_eq!(manifest.status(), 200);
    assert_eq!(
        manifest
            .headers()
            .get("content-type")
            .and_then(|v| v.to_str().ok()),
        Some("application/manifest+json")
    );
    let body: serde_json::Value = manifest.json().await.expect("manifest json");
    assert_eq!(body["name"], "Draft Assistant");
    assert_eq!(body["display"], "standalone");
    let worker = get("/static/sw.js").await.expect("worker");
    assert_eq!(worker.status(), 200);
    // The failure this prevents: a worker served under /static/ may only
    // control /static/, and the page at / could not be installed.
    assert_eq!(
        worker
            .headers()
            .get("service-worker-allowed")
            .and_then(|v| v.to_str().ok()),
        Some("/")
    );
    let csp = worker
        .headers()
        .get("content-security-policy")
        .and_then(|v| v.to_str().ok())
        .unwrap_or("");
    assert!(csp.contains("manifest-src 'self'"), "{csp}");
    assert!(csp.contains("worker-src 'self'"), "{csp}");
    let icon = get("/static/icon.svg").await.expect("icon");
    assert_eq!(
        icon.headers()
            .get("content-type")
            .and_then(|v| v.to_str().ok()),
        Some("image/svg+xml")
    );
    let page = get("/").await.expect("page").text().await.expect("html");
    assert!(page.contains(r#"<link rel="manifest" href="/static/manifest.webmanifest" />"#));
    assert!(page.contains(r#"<script src="/static/pwa.js"></script>"#));
}
