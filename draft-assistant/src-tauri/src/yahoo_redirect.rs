//! The loopback half of [`crate::yahoo_oauth`]: the HTTP request the browser
//! makes to `http://localhost:<port>` after the user approves the app.
//!
//! A child module of `yahoo_oauth` rather than a peer, because it is only ever
//! reached through it — `yahoo_oauth` re-exports everything public here, so
//! callers spell it `yahoo_oauth::catch_redirect` either way.
//!
//! Plain HTTP, not HTTPS: a loopback listener has no certificate anyone could
//! validate, and Yahoo accepts an `http://localhost` redirect for exactly that
//! reason. The code never leaves the machine.
//!
//! The listener accepts connections until one carries the code. It used to
//! take exactly one: browsers open speculative connections to a host they are
//! about to navigate to, and fetch `/favicon.ico` on their own, so the one
//! `accept` regularly went to a connection that said nothing and the real
//! redirect, a moment behind it, found nobody listening.

use super::AuthError;
use std::io::{Read, Write};
use std::net::{TcpListener, TcpStream};
use std::sync::atomic::{AtomicBool, Ordering};
use std::time::{Duration, Instant};

/// How long [`catch_redirect`] waits for the browser before giving up. Long
/// enough for a Yahoo login with two-factor on the end of it, short enough
/// that a user who wandered off does not leave a socket bound forever.
pub const REDIRECT_WAIT: Duration = Duration::from_secs(300);
/// Once the browser has connected, the request itself is a few hundred bytes
/// and arrives at once; a connection that then says nothing is not the
/// redirect.
const REDIRECT_READ_WAIT: Duration = Duration::from_secs(10);
/// How often a waiting listener looks at its cancel flag and its deadline.
/// The standard library has no timed accept, so both are polled; 50ms of
/// latency on a step the user spends a minute on costs nothing.
const POLL: Duration = Duration::from_millis(50);

/// What the browser handed back on the loopback redirect.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct Redirect {
    pub code: String,
    pub state: String,
    /// Yahoo's `error` parameter, when the user refused the app. A request
    /// carrying one is Yahoo's final answer; one carrying neither a code nor
    /// an error is a stray (a favicon fetch, a speculative connection) and is
    /// answered and forgotten.
    pub error: Option<String>,
}

/// Take the redirect the browser makes to `http://localhost:<port>`.
///
/// Blocking, and deliberately so: it is called from a blocking task while the
/// user is over in their browser. The page it answers with is the only thing
/// they see, so it says the one useful thing and stops.
pub fn catch_redirect(port: u16) -> Result<Redirect, AuthError> {
    catch_redirect_within(port, REDIRECT_WAIT)
}

/// [`catch_redirect`] with the wait named. Only a test wants this: it is how
/// the "the browser never came back" path is proved without the suite sitting
/// through the five minutes the app itself waits.
pub fn catch_redirect_within(port: u16, wait: Duration) -> Result<Redirect, AuthError> {
    let listener = TcpListener::bind(("127.0.0.1", port))
        .map_err(|e| AuthError::Invalid(format!("could not listen on port {port}: {e}")))?;
    catch_redirect_on_within(listener, wait)
}

/// [`catch_redirect`] with the listener already bound — which is how a test
/// gets a port without racing for a fixed one.
pub fn catch_redirect_on(listener: TcpListener) -> Result<Redirect, AuthError> {
    catch_redirect_on_within(listener, REDIRECT_WAIT)
}

/// [`catch_redirect_on`] with the wait named and nothing able to cancel it.
pub fn catch_redirect_on_within(
    listener: TcpListener,
    wait: Duration,
) -> Result<Redirect, AuthError> {
    catch_redirect_on_within_unless(listener, wait, &AtomicBool::new(false))
}

/// The whole of the loopback catch, bounded at both ends and cancellable.
///
/// `cancel` is the caller's way of taking the listener down early: the user
/// closed the Connect dialog, or started a new sign-in that needs the port.
/// Set it and the next poll, at most [`POLL`] away, returns with an
/// [`AuthError::Invalid`] and the listener dropped. Without it the port stayed
/// bound for the full five minutes after the dialog was gone, and a second
/// Connect in that time failed with "port in use".
///
/// Neither wait used to exist, so a user who closed the browser tab instead of
/// approving left this blocked on `accept` for as long as the app ran.
pub fn catch_redirect_on_within_unless(
    listener: TcpListener,
    wait: Duration,
    cancel: &AtomicBool,
) -> Result<Redirect, AuthError> {
    listener
        .set_nonblocking(true)
        .map_err(|e| AuthError::Transport(format!("the listener could not be polled: {e}")))?;
    let deadline = Instant::now() + wait;
    loop {
        let socket = accept_until(&listener, deadline, cancel)?;
        let Some(redirect) = answer(socket, deadline, cancel) else {
            // A connection that sent no request head: a speculative browser
            // connection, or one the browser closed. Not the redirect.
            continue;
        };
        if !redirect.code.is_empty() {
            return Ok(redirect);
        }
        if let Some(error) = redirect.error {
            return Err(AuthError::Invalid(format!(
                "Yahoo did not authorize the app ({error}) — start Connect again"
            )));
        }
        // No code and no error: a favicon fetch or a bare `/`. Answered, and
        // the real redirect is still expected.
    }
}

/// Wait for the next connection, or for the deadline or the cancel flag.
fn accept_until(
    listener: &TcpListener,
    deadline: Instant,
    cancel: &AtomicBool,
) -> Result<TcpStream, AuthError> {
    loop {
        if cancel.load(Ordering::SeqCst) {
            return Err(cancelled());
        }
        match listener.accept() {
            Ok((socket, _)) => return Ok(socket),
            Err(e) if e.kind() == std::io::ErrorKind::WouldBlock => {
                let left = deadline.saturating_duration_since(Instant::now());
                if left.is_zero() {
                    return Err(AuthError::Transport(
                        "the browser never came back with a code — start Connect again".into(),
                    ));
                }
                std::thread::sleep(left.min(POLL));
            }
            Err(e) => {
                return Err(AuthError::Transport(format!(
                    "the browser never arrived: {e}"
                )))
            }
        }
    }
}

fn cancelled() -> AuthError {
    AuthError::Invalid("the Yahoo sign-in was cancelled before the browser came back".into())
}

/// Read one request head off `socket` and answer it. `None` when nothing
/// arrived: the connection said nothing for [`REDIRECT_READ_WAIT`], was
/// closed, or the flow was cancelled or timed out while it was being read.
fn answer(mut socket: TcpStream, deadline: Instant, cancel: &AtomicBool) -> Option<Redirect> {
    // Short reads, polled: a connected browser that says nothing must not
    // hold the listener past a cancel, and the one that does speak sends the
    // whole GET at once.
    if socket
        .set_nonblocking(false)
        .and_then(|()| socket.set_read_timeout(Some(POLL)))
        .is_err()
    {
        return None;
    }
    let give_up = (Instant::now() + REDIRECT_READ_WAIT).min(deadline);
    let mut request = Vec::new();
    let mut chunk = [0u8; 4096];
    // The head is all there is: the browser sends a GET with no body.
    while !request.windows(4).any(|w| w == b"\r\n\r\n") {
        if cancel.load(Ordering::SeqCst) || Instant::now() >= give_up {
            return None;
        }
        match socket.read(&mut chunk) {
            Ok(0) => return None,
            Ok(n) => request.extend_from_slice(&chunk[..n]),
            Err(e)
                if matches!(
                    e.kind(),
                    std::io::ErrorKind::WouldBlock | std::io::ErrorKind::TimedOut
                ) => {}
            Err(_) => return None,
        }
    }
    let head = String::from_utf8_lossy(&request);
    let target = head.split_whitespace().nth(1).unwrap_or("/");
    let redirect = parse_redirect(target);
    let page = if redirect.code.is_empty() {
        "<!doctype html><title>Yahoo</title><p>No authorization code arrived. Try connecting again."
    } else {
        "<!doctype html><title>Yahoo</title><p>Connected. You can close this tab."
    };
    let response = format!(
        "HTTP/1.1 200 OK\r\nContent-Type: text/html; charset=utf-8\r\nContent-Length: {}\r\nConnection: close\r\n\r\n{page}",
        page.len()
    );
    let _ = socket.write_all(response.as_bytes());
    let _ = socket.flush();
    let _ = socket.shutdown(std::net::Shutdown::Write);
    Some(redirect)
}

/// Pull `code`, `state` and `error` out of the request target
/// (`/?code=..&state=..`).
pub fn parse_redirect(target: &str) -> Redirect {
    let query = target.split_once('?').map(|(_, q)| q).unwrap_or("");
    let mut redirect = Redirect::default();
    for pair in query.split('&') {
        let Some((name, value)) = pair.split_once('=') else {
            continue;
        };
        match name {
            "code" => redirect.code = decode(value),
            "state" => redirect.state = decode(value),
            "error" => redirect.error = Some(decode(value)),
            _ => {}
        }
    }
    redirect
}

/// The percent-decoding half of [`encode`], enough for a query value.
///
/// Works on bytes from start to finish. It used to slice the `str` two
/// characters past each `%`, which is a panic when the second of them is the
/// middle of a multi-byte character: `%aé`, on a query anybody can send to
/// the listener, took the whole sign-in down with it.
fn decode(raw: &str) -> String {
    let bytes = raw.as_bytes();
    let mut out = Vec::with_capacity(bytes.len());
    let mut index = 0;
    while index < bytes.len() {
        match bytes[index] {
            b'%' => match hex_pair(bytes.get(index + 1), bytes.get(index + 2)) {
                Some(byte) => {
                    out.push(byte);
                    index += 3;
                }
                None => {
                    out.push(b'%');
                    index += 1;
                }
            },
            b'+' => {
                out.push(b' ');
                index += 1;
            }
            byte => {
                out.push(byte);
                index += 1;
            }
        }
    }
    String::from_utf8_lossy(&out).into_owned()
}

/// The byte two hex digits spell, or `None` when either is missing or not a
/// hex digit.
fn hex_pair(high: Option<&u8>, low: Option<&u8>) -> Option<u8> {
    let high = (*high? as char).to_digit(16)?;
    let low = (*low? as char).to_digit(16)?;
    Some((high * 16 + low) as u8)
}
