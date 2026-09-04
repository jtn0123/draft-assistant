//! The loopback half of [`crate::yahoo_oauth`]: the one HTTP request the
//! browser makes to `http://localhost:<port>` after the user approves the app.
//!
//! A child module of `yahoo_oauth` rather than a peer, because it is only ever
//! reached through it — `yahoo_oauth` re-exports everything public here, so
//! callers spell it `yahoo_oauth::catch_redirect` either way.
//!
//! Plain HTTP, not HTTPS: a loopback listener has no certificate anyone could
//! validate, and Yahoo accepts an `http://localhost` redirect for exactly that
//! reason. The code never leaves the machine.

use super::AuthError;
use std::io::{Read, Write};
use std::net::TcpListener;
use std::time::Duration;

/// How long [`catch_redirect`] waits for the browser before giving up. Long
/// enough for a Yahoo login with two-factor on the end of it, short enough
/// that a user who wandered off does not leave a socket bound forever.
pub const REDIRECT_WAIT: Duration = Duration::from_secs(300);
/// Once the browser has connected, the request itself is a few hundred bytes
/// and arrives at once; a connection that then says nothing is not the
/// redirect.
const REDIRECT_READ_WAIT: Duration = Duration::from_secs(10);

/// What the browser handed back on the loopback redirect.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct Redirect {
    pub code: String,
    pub state: String,
}

/// Take the one redirect the browser makes to `http://localhost:<port>`.
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

/// The whole of the loopback catch, bounded at both ends.
///
/// Neither wait used to exist, so a user who closed the browser tab instead of
/// approving left this blocked on `accept` for as long as the app ran. The
/// accept is polled rather than blocking because the standard library has no
/// timed accept; 50ms of latency on a step the user spends a minute on costs
/// nothing.
pub fn catch_redirect_on_within(
    listener: TcpListener,
    wait: Duration,
) -> Result<Redirect, AuthError> {
    let (mut socket, _) = accept_within(&listener, wait)?;
    // Back to blocking reads, but with a ceiling: the browser's GET arrives in
    // one piece, so anything slower than this is not the redirect.
    socket
        .set_nonblocking(false)
        .and_then(|()| socket.set_read_timeout(Some(REDIRECT_READ_WAIT)))
        .map_err(|e| AuthError::Transport(format!("the redirect could not be read: {e}")))?;
    let mut request = Vec::new();
    let mut chunk = [0u8; 4096];
    // The head is all there is: the browser sends a GET with no body.
    while !request.windows(4).any(|w| w == b"\r\n\r\n") {
        match socket.read(&mut chunk) {
            Ok(0) | Err(_) => break,
            Ok(n) => request.extend_from_slice(&chunk[..n]),
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
    if redirect.code.is_empty() {
        return Err(AuthError::Invalid(
            "the redirect carried no authorization code".into(),
        ));
    }
    Ok(redirect)
}

/// Wait up to `wait` for the one connection the browser makes.
fn accept_within(
    listener: &TcpListener,
    wait: Duration,
) -> Result<(std::net::TcpStream, std::net::SocketAddr), AuthError> {
    listener
        .set_nonblocking(true)
        .map_err(|e| AuthError::Transport(format!("the listener could not be polled: {e}")))?;
    let deadline = std::time::Instant::now() + wait;
    loop {
        match listener.accept() {
            Ok(pair) => return Ok(pair),
            Err(e) if e.kind() == std::io::ErrorKind::WouldBlock => {
                let left = deadline.saturating_duration_since(std::time::Instant::now());
                if left.is_zero() {
                    return Err(AuthError::Transport(
                        "the browser never came back with a code — start Connect again".into(),
                    ));
                }
                std::thread::sleep(left.min(Duration::from_millis(50)));
            }
            Err(e) => {
                return Err(AuthError::Transport(format!(
                    "the browser never arrived: {e}"
                )))
            }
        }
    }
}

/// Pull `code` and `state` out of the request target (`/?code=..&state=..`).
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
            _ => {}
        }
    }
    redirect
}

/// The percent-decoding half of [`encode`], enough for a query value.
fn decode(raw: &str) -> String {
    let bytes = raw.as_bytes();
    let mut out = Vec::with_capacity(bytes.len());
    let mut index = 0;
    while index < bytes.len() {
        match bytes[index] {
            b'%' if index + 2 < bytes.len() => {
                match u8::from_str_radix(&raw[index + 1..index + 3], 16) {
                    Ok(byte) => {
                        out.push(byte);
                        index += 3;
                    }
                    Err(_) => {
                        out.push(b'%');
                        index += 1;
                    }
                }
            }
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
