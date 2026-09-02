//! A standard-library stand-in for api.sleeper.app.
//!
//! `sleeper_host` lets a debug build send every Sleeper URL somewhere else,
//! which is how `scripts/replay-sleeper.mjs` replays a recorded draft. The
//! same door lets a test serve the whole wire surface — league, draft, picks,
//! the players dictionary, projections, rosters, matchups, scores — from a
//! socket on localhost, so the loaders run end to end against real bytes
//! without a single request leaving the machine.
//!
//! No stub-server crate is pulled in for this: the server is a listener, a
//! thread per connection, and a routing closure the test supplies.

use std::io::{Read, Write};
use std::net::TcpListener;
use std::sync::OnceLock;

/// What a route decided: an HTTP status and a JSON body.
pub type Reply = (u16, String);

/// Decides the reply for one request path (with its query string, if any).
/// Returning `None` is a 404, which is what Sleeper serves for an unknown id.
pub type Router = fn(&str) -> Option<Reply>;

/// Point every Sleeper URL in this test binary at a stub driven by `router`.
///
/// Idempotent and safe to call from every test in the file: the listener and
/// the host override are set up once, on whichever test gets there first.
/// `sleeper_host::host()` reads the environment exactly once too, so the
/// override has to be in place before the first request — calling this at the
/// top of each test is how that is guaranteed.
pub fn serve(router: Router) {
    static STARTED: OnceLock<()> = OnceLock::new();
    STARTED.get_or_init(|| {
        let listener = TcpListener::bind("127.0.0.1:0").expect("bind the stub");
        let addr = listener.local_addr().expect("stub address");
        std::env::set_var("DRAFT_ASSISTANT_SLEEPER_BASE", format!("http://{addr}"));
        std::thread::spawn(move || {
            for stream in listener.incoming().flatten() {
                std::thread::spawn(move || answer(stream, router));
            }
        });
    });
}

fn answer(mut socket: std::net::TcpStream, router: Router) {
    // Read the whole request head before replying. Closing a socket with
    // unread bytes on it makes the kernel send a reset, and the client sees
    // "connection reset" instead of the response this stub meant to serve.
    let mut request = Vec::new();
    let mut chunk = [0u8; 8192];
    while !request.windows(4).any(|w| w == b"\r\n\r\n") {
        match socket.read(&mut chunk) {
            Ok(0) | Err(_) => return,
            Ok(n) => request.extend_from_slice(&chunk[..n]),
        }
    }
    let head = String::from_utf8_lossy(&request);
    let path = head
        .split_whitespace()
        .nth(1)
        .map(str::to_string)
        .unwrap_or_default();
    let (status, body) = router(&path).unwrap_or_else(|| (404, "null".to_string()));
    let response = format!(
        "HTTP/1.1 {status} X\r\nContent-Type: application/json\r\nContent-Length: {}\r\nConnection: close\r\n\r\n{body}",
        body.len()
    );
    let _ = socket.write_all(response.as_bytes());
    let _ = socket.flush();
    let _ = socket.shutdown(std::net::Shutdown::Write);
}

/// A fresh, empty data directory for one engine, removed by the test when it
/// is done. Named after the test so a leftover directory says who left it.
pub fn scratch_dir(label: &str) -> std::path::PathBuf {
    let unique = format!(
        "draft-assistant-stub-{label}-{}-{}",
        std::process::id(),
        std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .expect("clock")
            .as_nanos()
    );
    std::env::temp_dir().join(unique)
}
