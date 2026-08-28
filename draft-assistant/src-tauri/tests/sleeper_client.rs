//! What the Sleeper client does when the network misbehaves: every request is
//! bounded, and the two multi-megabyte downloads are bounded differently.

mod timeout_tests {
    use draft_assistant_lib::sleeper::SleeperClient;
    use std::time::{Duration, Instant};

    // A username lookup used to go through `reqwest::get`, whose default
    // client never times out — a stalled connection left Setup on "Looking up
    // your Sleeper account…" forever. Every request must give up.
    #[tokio::test]
    async fn a_username_lookup_against_a_silent_server_fails_within_the_timeout() {
        let listener = std::net::TcpListener::bind("127.0.0.1:0").unwrap();
        let addr = listener.local_addr().unwrap();
        // Accept connections and hold them open without ever answering.
        let held = std::sync::Arc::new(std::sync::Mutex::new(Vec::new()));
        let sink = held.clone();
        std::thread::spawn(move || {
            for stream in listener.incoming().flatten() {
                sink.lock().unwrap().push(stream);
            }
        });
        let client = SleeperClient::with_base_url_and_timeouts(
            &format!("http://{addr}"),
            Duration::from_millis(300),
            Duration::from_millis(300),
            Duration::from_millis(300),
        );
        let started = Instant::now();
        let result = tokio::time::timeout(Duration::from_secs(2), client.user_id("mcsleeper26"))
            .await
            .expect("the lookup hung past 2s: no request timeout is applied");
        let error = result.expect_err("a silent server must be an error");
        assert!(
            error.contains("timed out") || error.contains("timeout"),
            "unexpected error text: {error}"
        );
        assert!(started.elapsed() < Duration::from_secs(2));
    }
}

mod large_transfer_tests {
    use draft_assistant_lib::sleeper::SleeperClient;
    use std::time::{Duration, Instant};

    fn silent_server() -> (
        std::net::SocketAddr,
        std::sync::Arc<std::sync::Mutex<Vec<std::net::TcpStream>>>,
    ) {
        let listener = std::net::TcpListener::bind("127.0.0.1:0").unwrap();
        let addr = listener.local_addr().unwrap();
        let held = std::sync::Arc::new(std::sync::Mutex::new(Vec::new()));
        let sink = held.clone();
        std::thread::spawn(move || {
            for stream in listener.incoming().flatten() {
                sink.lock().unwrap().push(stream);
            }
        });
        (addr, held)
    }

    // The player dictionary and the weekly projections are ~14 MB and ~18 MB.
    // reqwest's timeout is total transfer time, so the 8 s that keeps a poll
    // honest would cut them off on slow wifi — the one network the app is
    // guaranteed to meet on draft night.
    #[tokio::test]
    async fn the_big_downloads_outlive_the_ordinary_request_timeout() {
        let (addr, _held) = silent_server();
        let client = SleeperClient::with_base_url_and_timeouts(
            &format!("http://{addr}"),
            Duration::from_millis(200),
            Duration::from_millis(200),
            Duration::from_millis(1200),
        );

        let started = Instant::now();
        let error = client
            .players()
            .await
            .expect_err("a silent server must fail");
        let waited = started.elapsed();
        assert!(
            waited >= Duration::from_millis(600),
            "players() gave up after {waited:?} — it is still on the 200 ms cap"
        );
        assert!(
            waited < Duration::from_secs(3),
            "players() waited {waited:?}"
        );
        assert!(
            error.contains("timed out") || error.contains("timeout"),
            "{error}"
        );

        let started = Instant::now();
        client
            .weekly_projections(2026, 1)
            .await
            .expect_err("a silent server must fail");
        assert!(started.elapsed() >= Duration::from_millis(600));
    }

    #[tokio::test]
    async fn ordinary_requests_keep_the_short_cap() {
        let (addr, _held) = silent_server();
        let client = SleeperClient::with_base_url_and_timeouts(
            &format!("http://{addr}"),
            Duration::from_millis(200),
            Duration::from_millis(200),
            Duration::from_secs(30),
        );

        let started = Instant::now();
        client
            .picks("d1")
            .await
            .expect_err("a silent server must fail");
        let waited = started.elapsed();
        assert!(
            waited < Duration::from_millis(900),
            "a poll waited {waited:?} — it picked up the large-transfer cap"
        );
    }
}
