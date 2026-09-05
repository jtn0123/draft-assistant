//! The HTTP client every Anthropic call goes through, and the reservation that
//! stops two questions about one league from both passing a budget cap only
//! one of them fits under.
//!
//! Both live here rather than in `commands_chat.rs` because that file is at
//! the line cap.

use std::collections::HashSet;
use std::sync::{Mutex, OnceLock};
use std::time::Duration;

/// How long to wait for a socket to api.anthropic.com.
pub const CONNECT_TIMEOUT: Duration = Duration::from_secs(10);

/// How long one answer may take, start to finish.
///
/// Ask Claude used to borrow the Sleeper client, which talks to an API that
/// answers in milliseconds and gives up after eight seconds. Opus 5 at high
/// effort thinks for minutes, so a real question died as "could not reach the
/// Anthropic API" while it was still being answered — and the user was
/// charged for the tokens that arrived after the client had stopped reading.
pub const REQUEST_TIMEOUT: Duration = Duration::from_secs(600);

/// A client with the timeouts a model call needs. Public so the tests can
/// build an impatient one and show that the total timeout is what bites.
pub fn build_with(connect: Duration, total: Duration) -> reqwest::Client {
    reqwest::Client::builder()
        .user_agent("draft-assistant/0.1 (local second-screen tool)")
        .connect_timeout(connect)
        .timeout(total)
        .build()
        .expect("failed to build the Anthropic http client")
}

/// The client every Anthropic call uses. Built once: a client per question
/// throws away the connection pool and pays for a fresh TLS handshake each
/// turn. Cloning one is cheap — the clone shares the pool.
pub fn client() -> reqwest::Client {
    static CLIENT: OnceLock<reqwest::Client> = OnceLock::new();
    CLIENT
        .get_or_init(|| build_with(CONNECT_TIMEOUT, REQUEST_TIMEOUT))
        .clone()
}

fn in_flight() -> &'static Mutex<HashSet<String>> {
    static IN_FLIGHT: OnceLock<Mutex<HashSet<String>>> = OnceLock::new();
    IN_FLIGHT.get_or_init(|| Mutex::new(HashSet::new()))
}

/// A question in flight. Dropping it lets the next one through, so every
/// early return, every `?`, and every panic releases the claim.
#[derive(Debug)]
pub struct InFlight(String);

impl Drop for InFlight {
    fn drop(&mut self) {
        in_flight()
            .lock()
            .unwrap_or_else(|e| e.into_inner())
            .remove(&self.0);
    }
}

/// Claim `key` for one question, or refuse because one is already running.
///
/// The budget cap is read before a turn and written after it. Two questions
/// asked at the same moment therefore both read the spend from before either
/// of them, and both passed a cap with room for only one — the second one was
/// free. One question at a time per key closes that window.
pub fn reserve(key: &str) -> Result<InFlight, String> {
    let mut held = in_flight().lock().unwrap_or_else(|e| e.into_inner());
    if !held.insert(key.to_string()) {
        return Err(
            "another question is already being answered for this league — wait for it to finish"
                .to_string(),
        );
    }
    Ok(InFlight(key.to_string()))
}

#[cfg(test)]
mod tests {
    use super::*;

    /// Accept one request, wait, then answer. Fifteen lines of standard
    /// library rather than a stub-server crate, as elsewhere in this crate.
    fn slow_stub(delay: Duration) -> String {
        use std::io::{Read, Write};
        let listener = std::net::TcpListener::bind("127.0.0.1:0").expect("bind");
        let url = format!("http://{}/slow", listener.local_addr().expect("addr"));
        std::thread::spawn(move || {
            let (mut socket, _) = listener.accept().expect("accept");
            let mut chunk = [0u8; 8192];
            let _ = socket.read(&mut chunk);
            std::thread::sleep(delay);
            let _ = socket.write_all(
                b"HTTP/1.1 200 X\r\nContent-Type: application/json\r\nContent-Length: 2\r\nConnection: close\r\n\r\n{}",
            );
            let _ = socket.flush();
            let _ = socket.shutdown(std::net::Shutdown::Write);
        });
        url
    }

    /// The Sleeper client this path used to borrow gives up after eight
    /// seconds, which is less than one Opus turn at high effort.
    #[test]
    fn the_chat_timeout_is_minutes_rather_than_the_sleeper_clients_seconds() {
        assert!(
            REQUEST_TIMEOUT >= Duration::from_secs(600),
            "{REQUEST_TIMEOUT:?}"
        );
        assert!(REQUEST_TIMEOUT > Duration::from_secs(8) * 10);
        assert!(CONNECT_TIMEOUT >= Duration::from_secs(10));
    }

    /// The constants are only worth anything if they reach the client, so the
    /// same delay is served to a deliberately impatient client and to the one
    /// Ask Claude uses. The impatient one is what an eight-second budget looks
    /// like scaled down; the real one waits.
    #[tokio::test]
    async fn the_shared_client_outwaits_a_delay_that_a_short_timeout_dies_on() {
        let delay = Duration::from_millis(400);

        let impatient = build_with(CONNECT_TIMEOUT, Duration::from_millis(50));
        let error = impatient
            .get(slow_stub(delay))
            .send()
            .await
            .expect_err("50ms is not long enough for a 400ms answer");
        assert!(error.is_timeout(), "{error}");

        let response = client()
            .get(slow_stub(delay))
            .send()
            .await
            .expect("the chat client waits for a slow answer");
        assert!(response.status().is_success());
    }

    #[test]
    fn a_second_question_about_the_same_league_is_refused_while_one_is_running() {
        let held = reserve("draft.reserve-test").expect("the first question is accepted");
        let error = reserve("draft.reserve-test").expect_err("the second is refused");
        assert!(error.contains("already being answered"), "{error}");
        // Another league is its own claim and is not blocked by it.
        let _other = reserve("draft.reserve-other").expect("a different league is free");
        drop(held);
        reserve("draft.reserve-test").expect("the claim was released");
    }
}
