//! The HTTP client every Anthropic call goes through, and the reservation that
//! stops two questions about one league from both passing a budget cap only
//! one of them fits under.
//!
//! Both live here rather than in `commands_chat.rs` because that file is at
//! the line cap.

use std::collections::HashMap;
use std::sync::{Arc, Mutex, OnceLock};
use std::time::Duration;
use tokio::sync::watch;

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

/// The one way to stop a question that is being answered.
///
/// The model call holds a clone and races the stream against it; the Cancel
/// button reaches the same signal through [`cancel`], by the key the claim
/// was made under. A question that was never claimed, or one that has already
/// finished, has no signal to reach, so cancelling it is a no-op that says so.
#[derive(Debug)]
pub struct CancelSignal {
    cancelled: watch::Sender<bool>,
}

impl CancelSignal {
    fn new() -> Arc<Self> {
        Arc::new(Self {
            cancelled: watch::Sender::new(false),
        })
    }

    /// A signal nobody can pull: for the paths, and the tests, with no button.
    pub fn never() -> Arc<Self> {
        Self::new()
    }

    pub fn cancel(&self) {
        self.cancelled.send_replace(true);
    }

    pub fn is_cancelled(&self) -> bool {
        *self.cancelled.borrow()
    }

    /// Resolves once [`CancelSignal::cancel`] has been called, at once if it
    /// already was. Never resolves otherwise, which is what a `select!` arm
    /// that should lose to the answer needs.
    pub async fn cancelled(&self) {
        let mut seen = self.cancelled.subscribe();
        // The sender lives as long as `self`, so waiting cannot fail.
        let _ = seen.wait_for(|cancelled| *cancelled).await;
    }
}

/// A question in flight. Dropping it lets the next one through, so every
/// early return, every `?`, and every panic releases the claim.
#[derive(Debug)]
pub struct InFlight {
    key: String,
    signal: Arc<CancelSignal>,
    /// The registry the claim was made in, so dropping releases it there.
    claims: Arc<Mutex<Claims>>,
}

impl InFlight {
    /// The key this claim was made under.
    pub fn key(&self) -> &str {
        &self.key
    }

    /// The signal the model call should race against.
    pub fn signal(&self) -> Arc<CancelSignal> {
        self.signal.clone()
    }
}

impl Drop for InFlight {
    fn drop(&mut self) {
        self.claims
            .lock()
            .unwrap_or_else(|e| e.into_inner())
            .remove(&self.key);
    }
}

/// The sentence a second asker gets while a question about the same board is
/// still being answered. One sentence, whichever side asked: the desktop panel
/// shows it as an error turn, and the phone gets it as the reason on its 409.
pub const BUSY_MESSAGE: &str =
    "a question about this board is already being answered, wait for it to finish";

type Claims = HashMap<String, Arc<CancelSignal>>;

/// The questions in flight, one slot per spend key, and the signal that
/// stops each of them.
///
/// Owned by the app state rather than kept in a static: the desktop commands
/// and the companion server reach the same registry through the state they
/// share, and two servers built in one process (every test builds its own)
/// never see each other's claims.
#[derive(Debug, Default)]
pub struct InFlightClaims {
    held: Arc<Mutex<Claims>>,
}

impl InFlightClaims {
    /// Claim `key` for one question, or refuse because one is already running.
    ///
    /// The budget cap is read before a turn and written after it. Two
    /// questions asked at the same moment therefore both read the spend from
    /// before either of them, and both passed a cap with room for only one —
    /// the second one was free. One question at a time per key closes that
    /// window.
    pub fn reserve(&self, key: &str) -> Result<InFlight, String> {
        let mut held = self.held.lock().unwrap_or_else(|e| e.into_inner());
        if held.contains_key(key) {
            return Err(BUSY_MESSAGE.to_string());
        }
        let signal = CancelSignal::new();
        held.insert(key.to_string(), signal.clone());
        Ok(InFlight {
            key: key.to_string(),
            signal,
            claims: self.held.clone(),
        })
    }

    /// Stop the question claimed under `key`, if one is. True when there was
    /// one to stop; the answer itself reports back through its own call.
    pub fn cancel(&self, key: &str) -> bool {
        let signal = self
            .held
            .lock()
            .unwrap_or_else(|e| e.into_inner())
            .get(key)
            .cloned();
        match signal {
            Some(signal) => {
                signal.cancel();
                true
            }
            None => false,
        }
    }
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
        let claims = InFlightClaims::default();
        let held = claims
            .reserve("draft.reserve-test")
            .expect("the first question is accepted");
        let error = claims
            .reserve("draft.reserve-test")
            .expect_err("the second is refused");
        assert_eq!(error, BUSY_MESSAGE);
        // Another league is its own claim and is not blocked by it.
        let _other = claims
            .reserve("draft.reserve-other")
            .expect("a different league is free");
        drop(held);
        claims
            .reserve("draft.reserve-test")
            .expect("the claim was released");
    }

    /// Two registries are two registries: a claim in one is not a claim in
    /// the other. This is what lets two servers share one process.
    #[test]
    fn a_claim_in_one_registry_does_not_block_another() {
        let one = InFlightClaims::default();
        let two = InFlightClaims::default();
        let _held = one.reserve("draft.league").expect("free in one");
        two.reserve("draft.league").expect("and still free in two");
        assert!(!two.cancel("draft.league"), "two never saw one's claim");
    }

    /// The Cancel button reaches the call through the claim's key. Nothing to
    /// cancel is a plain `false`, not an error: the answer may have landed a
    /// moment before the click.
    #[tokio::test]
    async fn cancelling_by_key_pulls_the_signal_the_claim_handed_out() {
        let claims = InFlightClaims::default();
        assert!(
            !claims.cancel("draft.cancel-test"),
            "nothing is in flight yet"
        );
        let held = claims.reserve("draft.cancel-test").expect("claimed");
        let signal = held.signal();
        assert!(!signal.is_cancelled());
        let waiting = tokio::spawn({
            let signal = signal.clone();
            async move { signal.cancelled().await }
        });
        assert!(claims.cancel("draft.cancel-test"), "the claim was found");
        assert!(signal.is_cancelled());
        tokio::time::timeout(Duration::from_secs(2), waiting)
            .await
            .expect("the waiter woke")
            .expect("and did not panic");
        // Already cancelled: resolves at once rather than waiting for a
        // second pull that is never coming.
        tokio::time::timeout(Duration::from_millis(200), signal.cancelled())
            .await
            .expect("a cancelled signal resolves immediately");
        drop(held);
        assert!(
            !claims.cancel("draft.cancel-test"),
            "the claim is gone with the turn"
        );
    }

    #[tokio::test]
    async fn a_signal_nobody_pulls_never_resolves() {
        let signal = CancelSignal::never();
        assert!(
            tokio::time::timeout(Duration::from_millis(50), signal.cancelled())
                .await
                .is_err(),
            "resolved without being cancelled"
        );
    }
}
