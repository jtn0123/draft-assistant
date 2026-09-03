//! A standard-library stand-in for Yahoo — both hosts at once.
//!
//! `tests/stub/mod.rs` does this for Sleeper, but a Yahoo client needs more
//! than a path router: the tests here have to see the request (the `Bearer`
//! header, the `format=json` query, the `Basic` header and form body on a
//! refresh) and to answer differently the second time the same path is asked
//! for. So this stub records every request and takes a closure rather than a
//! function pointer, and it hands back its address instead of setting an
//! environment variable — one `YahooClient` per test, no shared global.
//!
//! No stub-server crate: a listener, a thread per connection, and a closure.

#![allow(dead_code)]

use std::collections::HashMap;
use std::io::{Read, Write};
use std::net::TcpListener;
use std::sync::{Arc, Mutex};
use std::time::Duration;

/// One request, as the stub saw it.
#[derive(Debug, Clone, Default)]
pub struct Request {
    pub method: String,
    /// Path and query, exactly as sent.
    pub target: String,
    /// Header names lowercased.
    pub headers: HashMap<String, String>,
    pub body: String,
}

impl Request {
    pub fn header(&self, name: &str) -> Option<&str> {
        self.headers.get(name).map(String::as_str)
    }

    /// The value of one `application/x-www-form-urlencoded` field.
    pub fn form(&self, name: &str) -> Option<String> {
        self.body.split('&').find_map(|pair| {
            let (key, value) = pair.split_once('=')?;
            (key == name).then(|| percent_decode(value))
        })
    }

    /// The path with the query taken off.
    pub fn path(&self) -> &str {
        self.target.split('?').next().unwrap_or(&self.target)
    }

    pub fn query(&self) -> &str {
        self.target.split_once('?').map(|(_, q)| q).unwrap_or("")
    }
}

fn percent_decode(raw: &str) -> String {
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

/// What the stub should do about one request.
#[derive(Debug, Clone)]
pub enum Reply {
    /// An HTTP status and a body.
    Json(u16, String),
    /// Accept the connection, answer nothing, hang up after `0` bytes — the
    /// shape of a request that times out.
    Hang(Duration),
}

impl Reply {
    pub fn ok(body: impl Into<String>) -> Self {
        Reply::Json(200, body.into())
    }

    pub fn status(status: u16, body: impl Into<String>) -> Self {
        Reply::Json(status, body.into())
    }
}

/// A running stub. Dropping it leaves the thread parked on `accept`, which
/// ends with the test binary; nothing here outlives the process.
pub struct Stub {
    address: String,
    seen: Arc<Mutex<Vec<Request>>>,
}

impl Stub {
    /// The base URL to build a client against — no trailing slash.
    pub fn base(&self) -> String {
        format!("http://{}", self.address)
    }

    /// Every request the stub has answered, in order.
    pub fn requests(&self) -> Vec<Request> {
        self.seen.lock().expect("stub lock").clone()
    }

    pub fn count(&self) -> usize {
        self.seen.lock().expect("stub lock").len()
    }

    /// The requests whose path contains `fragment`.
    pub fn matching(&self, fragment: &str) -> Vec<Request> {
        self.requests()
            .into_iter()
            .filter(|request| request.target.contains(fragment))
            .collect()
    }
}

/// Start a stub whose `router` decides every reply.
pub fn serve<F>(router: F) -> Stub
where
    F: Fn(&Request) -> Reply + Send + Sync + 'static,
{
    let listener = TcpListener::bind("127.0.0.1:0").expect("bind the stub");
    let address = listener.local_addr().expect("stub address").to_string();
    let seen = Arc::new(Mutex::new(Vec::new()));
    let router = Arc::new(router);
    let recorder = Arc::clone(&seen);
    std::thread::spawn(move || {
        for stream in listener.incoming().flatten() {
            let router = Arc::clone(&router);
            let recorder = Arc::clone(&recorder);
            std::thread::spawn(move || answer(stream, &*router, &recorder));
        }
    });
    Stub { address, seen }
}

fn answer<F>(mut socket: std::net::TcpStream, router: &F, seen: &Mutex<Vec<Request>>)
where
    F: Fn(&Request) -> Reply,
{
    let Some(request) = read_request(&mut socket) else {
        return;
    };
    let reply = router(&request);
    seen.lock().expect("stub lock").push(request);
    match reply {
        Reply::Json(status, body) => {
            let response = format!(
                "HTTP/1.1 {status} X\r\nContent-Type: application/json\r\nContent-Length: {}\r\nConnection: close\r\n\r\n{body}",
                body.len()
            );
            let _ = socket.write_all(response.as_bytes());
            let _ = socket.flush();
            let _ = socket.shutdown(std::net::Shutdown::Write);
        }
        Reply::Hang(duration) => {
            // Hold the connection open with nothing on it, which is what the
            // client's read timeout is for.
            std::thread::sleep(duration);
        }
    }
}

/// Read one whole request: the head, then as much body as `Content-Length`
/// promised. Answering before the body has arrived makes the kernel reset the
/// connection, and the client would see that instead of the reply.
fn read_request(socket: &mut std::net::TcpStream) -> Option<Request> {
    let mut raw = Vec::new();
    let mut chunk = [0u8; 8192];
    let mut split = None;
    loop {
        if split.is_none() {
            split = raw.windows(4).position(|w| w == b"\r\n\r\n");
        }
        if let Some(at) = split {
            let head = String::from_utf8_lossy(&raw[..at]).to_string();
            if raw.len() - (at + 4) >= content_length(&head) {
                let body = String::from_utf8_lossy(&raw[at + 4..]).to_string();
                return Some(parse_head(&head, body));
            }
        }
        match socket.read(&mut chunk) {
            Ok(0) | Err(_) => return None,
            Ok(n) => raw.extend_from_slice(&chunk[..n]),
        }
    }
}

fn content_length(head: &str) -> usize {
    head.lines()
        .find_map(|line| {
            let (name, value) = line.split_once(':')?;
            name.eq_ignore_ascii_case("content-length")
                .then(|| value.trim().parse::<usize>().ok())
                .flatten()
        })
        .unwrap_or(0)
}

fn parse_head(head: &str, body: String) -> Request {
    let mut lines = head.lines();
    let start = lines.next().unwrap_or_default();
    let mut parts = start.split_whitespace();
    let method = parts.next().unwrap_or_default().to_string();
    let target = parts.next().unwrap_or_default().to_string();
    let headers = lines
        .filter_map(|line| {
            let (name, value) = line.split_once(':')?;
            Some((name.trim().to_ascii_lowercase(), value.trim().to_string()))
        })
        .collect();
    Request {
        method,
        target,
        headers,
        body,
    }
}

/// A counter a router closure can use to answer differently each time.
#[derive(Clone, Default)]
pub struct Hits(Arc<Mutex<HashMap<String, u32>>>);

impl Hits {
    pub fn new() -> Self {
        Self::default()
    }

    /// How many times `label` has been asked for, counting this one, 1-based.
    pub fn bump(&self, label: &str) -> u32 {
        let mut map = self.0.lock().expect("hits lock");
        let entry = map.entry(label.to_string()).or_insert(0);
        *entry += 1;
        *entry
    }

    pub fn seen(&self, label: &str) -> u32 {
        self.0
            .lock()
            .expect("hits lock")
            .get(label)
            .copied()
            .unwrap_or(0)
    }
}
