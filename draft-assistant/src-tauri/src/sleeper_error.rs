//! What a Sleeper request can fail with.
//!
//! The transport layer already knows whether a failure is worth another try —
//! a dropped connection or a 5xx is, a 404 or a malformed body is not. Before
//! this type that bit was computed inside `get_json_once` and thrown away when
//! the error flattened to a `String`, so no caller above the retry loop could
//! tell a terminal failure from a transient one without matching message text.
//!
//! `Display` deliberately renders the exact wording the Tauri boundary used to
//! build by hand, because those strings reach the UI: the type changed, the
//! sentences the user reads did not.

use std::fmt;

/// A failed read from Sleeper.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum SleeperError {
    /// The request was never made: the caller's input could not be put in a URL.
    Invalid(String),
    /// Sleeper answered successfully with an empty body — the thing asked for
    /// does not exist. Carries the whole sentence because only the caller knows
    /// what it was looking for.
    NotFound(String),
    /// A non-success status line.
    Http {
        status: reqwest::StatusCode,
        url: String,
    },
    /// The request never completed: DNS, connect, TLS, timeout, reset.
    Transport { url: String, detail: String },
    /// The response arrived but did not deserialize into the expected shape.
    Decode { url: String, detail: String },
}

impl SleeperError {
    /// Whether repeating the identical request could plausibly succeed.
    ///
    /// Transport failures and 5xx are the blips Sleeper throws during Sunday
    /// traffic. A 4xx means the URL is wrong and will stay wrong, a decode
    /// failure means the payload shape is wrong and will stay wrong, and an
    /// invalid input never reached the network at all.
    pub fn retryable(&self) -> bool {
        match self {
            SleeperError::Transport { .. } => true,
            SleeperError::Http { status, .. } => status.is_server_error(),
            SleeperError::Invalid(_) | SleeperError::NotFound(_) | SleeperError::Decode { .. } => {
                false
            }
        }
    }
}

impl fmt::Display for SleeperError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            SleeperError::Invalid(message) | SleeperError::NotFound(message) => {
                f.write_str(message)
            }
            SleeperError::Http { status, url } => write!(f, "HTTP {status} for {url}"),
            SleeperError::Transport { url, detail } => write!(f, "request failed: {url}: {detail}"),
            SleeperError::Decode { url, detail } => write!(f, "bad JSON from {url}: {detail}"),
        }
    }
}

impl std::error::Error for SleeperError {}

/// The Tauri IPC surface is `Result<_, String>`, so somewhere a `SleeperError`
/// has to become text. This is that somewhere: every command-layer call site
/// spells the conversion `.map_err(to_message)`, which keeps the boundary
/// greppable and keeps the sentence the user reads in exactly one place.
pub fn to_message(error: SleeperError) -> String {
    error.to_string()
}

#[cfg(test)]
mod tests {
    use super::SleeperError;
    use reqwest::StatusCode;

    fn http(code: u16) -> SleeperError {
        SleeperError::Http {
            status: StatusCode::from_u16(code).unwrap(),
            url: "https://api.sleeper.app/v1/league/1".to_string(),
        }
    }

    #[test]
    fn transport_failures_are_worth_another_try() {
        let error = SleeperError::Transport {
            url: "https://api.sleeper.app/v1/state/nfl".to_string(),
            detail: "connection closed".to_string(),
        };
        assert!(error.retryable());
    }

    #[test]
    fn server_errors_are_retryable_and_client_errors_are_not() {
        for code in [500, 502, 503, 504] {
            assert!(http(code).retryable(), "HTTP {code} should be retryable");
        }
        for code in [400, 401, 403, 404, 410, 429] {
            assert!(!http(code).retryable(), "HTTP {code} should be terminal");
        }
    }

    #[test]
    fn not_found_and_decode_failures_are_terminal() {
        assert!(!SleeperError::NotFound("league 1 not found".to_string()).retryable());
        assert!(!SleeperError::Decode {
            url: "https://api.sleeper.app/v1/players/nfl".to_string(),
            detail: "expected value".to_string(),
        }
        .retryable());
        assert!(
            !SleeperError::Invalid("'a b' is not a valid Sleeper username".to_string()).retryable()
        );
    }

    /// The wording below is what the command layer produced before this type
    /// existed; the frontend renders these verbatim, so they are frozen.
    #[test]
    fn display_matches_the_strings_the_frontend_already_shows() {
        assert_eq!(
            SleeperError::Transport {
                url: "https://api.sleeper.app/v1/state/nfl".to_string(),
                detail: "error sending request".to_string(),
            }
            .to_string(),
            "request failed: https://api.sleeper.app/v1/state/nfl: error sending request"
        );
        assert_eq!(
            http(503).to_string(),
            "HTTP 503 Service Unavailable for https://api.sleeper.app/v1/league/1"
        );
        assert_eq!(
            http(404).to_string(),
            "HTTP 404 Not Found for https://api.sleeper.app/v1/league/1"
        );
        assert_eq!(
            SleeperError::Decode {
                url: "https://api.sleeper.app/v1/players/nfl".to_string(),
                detail: "expected value at line 1 column 1".to_string(),
            }
            .to_string(),
            "bad JSON from https://api.sleeper.app/v1/players/nfl: expected value at line 1 column 1"
        );
        assert_eq!(
            SleeperError::NotFound("league 42 not found (Sleeper returned null)".to_string())
                .to_string(),
            "league 42 not found (Sleeper returned null)"
        );
        assert_eq!(
            SleeperError::Invalid("'bad name' is not a valid Sleeper username".to_string())
                .to_string(),
            "'bad name' is not a valid Sleeper username"
        );
    }
}

/// The retry loop in `SleeperClient::get_json` is the reason this type exists,
/// so it is exercised here against a real (tiny, local) HTTP server: what
/// matters is not that `retryable()` returns the right bool but that the loop
/// obeys it, and only counting TCP connections can show that.
#[cfg(test)]
mod retry_loop_tests {
    use crate::sleeper::SleeperClient;
    use crate::sleeper_error::SleeperError;
    use std::io::{Read, Write};
    use std::net::TcpListener;
    use std::sync::atomic::{AtomicUsize, Ordering};
    use std::sync::Arc;

    /// Serve `response` verbatim to every connection, counting connections.
    /// `Connection: close` keeps reqwest from pooling, so one attempt is
    /// exactly one accepted connection.
    fn stub(response: &'static str) -> (String, Arc<AtomicUsize>) {
        let listener = TcpListener::bind("127.0.0.1:0").expect("bind stub server");
        let url = format!("http://{}/stub", listener.local_addr().unwrap());
        let hits = Arc::new(AtomicUsize::new(0));
        let counter = Arc::clone(&hits);
        std::thread::spawn(move || {
            for stream in listener.incoming() {
                let Ok(mut stream) = stream else { break };
                counter.fetch_add(1, Ordering::SeqCst);
                let mut buffer = [0u8; 2048];
                let _ = stream.read(&mut buffer);
                let _ = stream.write_all(response.as_bytes());
                let _ = stream.flush();
            }
        });
        (url, hits)
    }

    async fn fetch(url: &str) -> SleeperError {
        SleeperClient::without_proxy()
            .get_json::<serde_json::Value>(url)
            .await
            .expect_err("the stub server never returns a usable body")
    }

    #[tokio::test]
    async fn a_server_error_is_tried_again_up_to_the_attempt_limit() {
        let (url, hits) = stub(
            "HTTP/1.1 503 Service Unavailable\r\nContent-Length: 0\r\nConnection: close\r\n\r\n",
        );
        let error = fetch(&url).await;
        assert!(error.retryable(), "503 should be retryable: {error}");
        assert!(matches!(error, SleeperError::Http { .. }));
        assert_eq!(
            hits.load(Ordering::SeqCst),
            3,
            "a retryable failure should use every attempt"
        );
    }

    #[tokio::test]
    async fn a_missing_resource_is_not_asked_for_twice() {
        let (url, hits) =
            stub("HTTP/1.1 404 Not Found\r\nContent-Length: 0\r\nConnection: close\r\n\r\n");
        let error = fetch(&url).await;
        assert!(!error.retryable(), "404 should be terminal: {error}");
        assert_eq!(
            hits.load(Ordering::SeqCst),
            1,
            "a terminal failure should stop after the first attempt"
        );
    }

    #[tokio::test]
    async fn a_malformed_body_is_not_asked_for_twice() {
        let (url, hits) = stub(
            "HTTP/1.1 200 OK\r\nContent-Type: application/json\r\nContent-Length: 11\r\nConnection: close\r\n\r\nnot json at",
        );
        let error = fetch(&url).await;
        assert!(matches!(error, SleeperError::Decode { .. }), "{error}");
        assert!(!error.retryable());
        assert_eq!(hits.load(Ordering::SeqCst), 1);
    }

    /// The players dictionary comes back as bytes so it can be parsed off the
    /// runtime, and that path has to keep the same retry policy as `get_json`.
    #[tokio::test]
    async fn the_raw_body_fetch_returns_what_the_server_sent() {
        let (url, hits) = stub(
            "HTTP/1.1 200 OK\r\nContent-Type: application/json\r\nContent-Length: 9\r\nConnection: close\r\n\r\n{\"a\": 1}\n",
        );
        let body = SleeperClient::without_proxy()
            .get_bytes_within(&url, None)
            .await
            .expect("a 200 with a body");
        assert_eq!(String::from_utf8(body).unwrap(), "{\"a\": 1}\n");
        assert_eq!(hits.load(Ordering::SeqCst), 1);
    }

    #[tokio::test]
    async fn the_raw_body_fetch_retries_a_server_error_and_stops_at_a_404() {
        let (url, hits) = stub(
            "HTTP/1.1 503 Service Unavailable\r\nContent-Length: 0\r\nConnection: close\r\n\r\n",
        );
        let error = SleeperClient::without_proxy()
            .get_bytes_within(&url, None)
            .await
            .expect_err("503 is not a body");
        assert!(error.retryable(), "{error}");
        assert_eq!(hits.load(Ordering::SeqCst), 3);

        let (url, hits) =
            stub("HTTP/1.1 404 Not Found\r\nContent-Length: 0\r\nConnection: close\r\n\r\n");
        SleeperClient::without_proxy()
            .get_bytes_within(&url, None)
            .await
            .expect_err("404 is not a body");
        assert_eq!(hits.load(Ordering::SeqCst), 1);
    }

    #[tokio::test]
    async fn a_refused_connection_is_a_retryable_transport_failure() {
        // Bound only to reserve a port, then dropped: connections are refused.
        let port = TcpListener::bind("127.0.0.1:0")
            .and_then(|l| l.local_addr())
            .map(|a| a.port())
            .expect("reserve a port");
        let error = fetch(&format!("http://127.0.0.1:{port}/stub")).await;
        assert!(matches!(error, SleeperError::Transport { .. }), "{error}");
        assert!(error.retryable());
    }
}
