//! The one-shot HTTP stub and the event builders behind `chat_wire_tests.rs`.
//!
//! The stub is built from the standard library: no stub-server crate is
//! pulled in for it, because the server is a few dozen lines and the wire
//! tests are its only caller. It answers exactly one request, as server-sent
//! events on a 200 and as JSON otherwise, and can be told to stop halfway.

use super::*;

/// A context to send. What it says does not matter to these tests; that it is
/// the two-halves shape the request builder takes does.
pub(super) fn context() -> crate::chat_context::SplitContext {
    crate::chat_context::SplitContext {
        stable: "the league".to_string(),
        volatile: "the board\n".to_string(),
    }
}

/// The hand-written stream under `tests/fixtures/`, the same one the parser's
/// own tests read. Here it is served over a socket.
pub(super) const FIXTURE: &str = include_str!("../tests/fixtures/chat_stream.sse");

// ---------- SSE bodies, built the way the API sends them ----------

pub(super) fn event(data: &str) -> String {
    let name = serde_json::from_str::<serde_json::Value>(data)
        .ok()
        .and_then(|v| v["type"].as_str().map(str::to_string))
        .unwrap_or_default();
    format!("event: {name}\ndata: {data}\n\n")
}

pub(super) fn message_start(model: &str, usage: &str) -> String {
    event(&format!(
        r#"{{"type":"message_start","message":{{"model":"{model}","usage":{usage}}}}}"#
    ))
}

pub(super) fn text_block(index: usize, text: &str) -> String {
    let text = serde_json::to_string(text).expect("a JSON string");
    event(&format!(
        r#"{{"type":"content_block_start","index":{index},"content_block":{{"type":"text","text":""}}}}"#
    )) + &event(&format!(
        r#"{{"type":"content_block_delta","index":{index},"delta":{{"type":"text_delta","text":{text}}}}}"#
    )) + &event(&format!(
        r#"{{"type":"content_block_stop","index":{index}}}"#
    ))
}

pub(super) fn thinking_block(index: usize) -> String {
    event(&format!(
        r#"{{"type":"content_block_start","index":{index},"content_block":{{"type":"thinking","thinking":""}}}}"#
    )) + &event(&format!(
        r#"{{"type":"content_block_delta","index":{index},"delta":{{"type":"thinking_delta","thinking":"weighing tiers"}}}}"#
    ))
}

pub(super) fn message_delta(stop: &str, usage: &str) -> String {
    event(&format!(
        r#"{{"type":"message_delta","delta":{{"stop_reason":"{stop}","stop_sequence":null}},"usage":{usage}}}"#
    )) + &event(r#"{"type":"message_stop"}"#)
}

/// A whole answer: one text block, ordinary stop, the given usage.
pub(super) fn answer(text: &str, stop: &str, usage_in: &str, output_tokens: u32) -> String {
    message_start("claude-opus-5", usage_in)
        + &text_block(0, text)
        + &message_delta(stop, &format!(r#"{{"output_tokens":{output_tokens}}}"#))
}

// ---------- the stub server ----------

/// One canned answer the stub serves to one request.
pub(super) struct Canned {
    pub(super) status: u16,
    pub(super) body: String,
    /// Extra response headers, `retry-after` being the one that matters.
    pub(super) headers: Vec<(&'static str, String)>,
    /// Send only the first half of the body, wait this long, and drop the
    /// connection.
    pub(super) stall: Option<std::time::Duration>,
}

impl Canned {
    pub(super) fn new(status: u16, body: impl Into<String>) -> Self {
        Canned {
            status,
            body: body.into(),
            headers: Vec::new(),
            stall: None,
        }
    }

    pub(super) fn header(mut self, name: &'static str, value: impl Into<String>) -> Self {
        self.headers.push((name, value.into()));
        self
    }
}

/// Serve `body` with `status` to exactly one request, and return the URL to
/// send it to. The thread ends with the response.
pub(super) fn stub_server(status: u16, body: String) -> String {
    let (url, _) = stub_with(status, body, std::time::Duration::ZERO, false);
    url
}

/// The same answer to `times` requests in a row: what a rate limit that does
/// not lift looks like to a client that retries.
pub(super) fn stub_repeated(status: u16, body: &str, times: usize) -> String {
    let (url, _) = stub_sequence((0..times).map(|_| Canned::new(status, body)).collect());
    url
}

/// Capture the request, answer with `status` and `body`, and — when `stall`
/// is set — send only the first half of the body, wait `pause`, and drop the
/// connection. The receiver hands back what the client sent as `(head, body)`.
pub(super) fn stub_with(
    status: u16,
    body: String,
    pause: std::time::Duration,
    stall: bool,
) -> (String, std::sync::mpsc::Receiver<(String, String)>) {
    stub_sequence(vec![Canned {
        status,
        body,
        headers: Vec::new(),
        stall: stall.then_some(pause),
    }])
}

/// The general stub: one connection per canned answer, in order, each request
/// captured and sent back as `(head, body)`. The thread ends with the last
/// answer, so a request past the end is refused at the socket.
pub(super) fn stub_sequence(
    answers: Vec<Canned>,
) -> (String, std::sync::mpsc::Receiver<(String, String)>) {
    use std::io::{Read, Write};
    let listener = std::net::TcpListener::bind("127.0.0.1:0").expect("bind");
    let url = format!(
        "http://{}/v1/messages",
        listener.local_addr().expect("addr")
    );
    let (send, recv) = std::sync::mpsc::channel();
    std::thread::spawn(move || {
        for canned in answers {
            let (mut socket, _) = listener.accept().expect("accept");
            // Drain the whole request before answering. Closing a socket with
            // unread bytes on it makes the kernel send a reset instead of a
            // FIN, and a client still writing its body then sees "connection
            // reset" in place of the status and body this stub meant to serve.
            let mut request = Vec::new();
            let mut chunk = [0u8; 8192];
            loop {
                match socket.read(&mut chunk) {
                    Ok(0) | Err(_) => break,
                    Ok(n) => request.extend_from_slice(&chunk[..n]),
                }
                if request_is_complete(&request) {
                    break;
                }
            }
            let whole = String::from_utf8_lossy(&request).into_owned();
            let (head, sent) = whole.split_once("\r\n\r\n").unwrap_or((whole.as_str(), ""));
            let _ = send.send((head.to_string(), sent.to_string()));
            let content_type = if canned.status == 200 {
                "text/event-stream"
            } else {
                "application/json"
            };
            let extra: String = canned
                .headers
                .iter()
                .map(|(name, value)| format!("{name}: {value}\r\n"))
                .collect();
            let head = format!(
                "HTTP/1.1 {} X\r\nContent-Type: {content_type}\r\nContent-Length: {}\r\n{extra}Connection: close\r\n\r\n",
                canned.status,
                canned.body.len()
            );
            let _ = socket.write_all(head.as_bytes());
            if let Some(pause) = canned.stall {
                let half = canned.body.len() / 2;
                let _ = socket.write_all(&canned.body.as_bytes()[..half]);
                let _ = socket.flush();
                std::thread::sleep(pause);
                // Dropped short: the client never gets the rest.
                continue;
            }
            let _ = socket.write_all(canned.body.as_bytes());
            let _ = socket.flush();
            // Half-close: the client reads a clean end of stream, not a reset.
            let _ = socket.shutdown(std::net::Shutdown::Write);
        }
    });
    (url, recv)
}

/// A 200 whose body arrives in pieces, `pause` apart: what a real answer does,
/// and the only way to show that a watcher is told about one while it is still
/// being written rather than once at the end.
pub(super) fn stub_dribbled(parts: Vec<String>, pause: std::time::Duration) -> String {
    use std::io::{Read, Write};
    let listener = std::net::TcpListener::bind("127.0.0.1:0").expect("bind");
    let url = format!(
        "http://{}/v1/messages",
        listener.local_addr().expect("addr")
    );
    std::thread::spawn(move || {
        let (mut socket, _) = listener.accept().expect("accept");
        let mut request = Vec::new();
        let mut chunk = [0u8; 8192];
        while !request_is_complete(&request) {
            match socket.read(&mut chunk) {
                Ok(0) | Err(_) => break,
                Ok(n) => request.extend_from_slice(&chunk[..n]),
            }
        }
        let length: usize = parts.iter().map(String::len).sum();
        let head = format!(
            "HTTP/1.1 200 X\r\nContent-Type: text/event-stream\r\nContent-Length: {length}\r\nConnection: close\r\n\r\n"
        );
        let _ = socket.write_all(head.as_bytes());
        let _ = socket.flush();
        for part in parts {
            std::thread::sleep(pause);
            let _ = socket.write_all(part.as_bytes());
            let _ = socket.flush();
        }
        let _ = socket.shutdown(std::net::Shutdown::Write);
    });
    url
}

/// True once `bytes` holds the request head and as many body bytes as its
/// `Content-Length` promised. A head with no length is complete on its own.
pub(super) fn request_is_complete(bytes: &[u8]) -> bool {
    let Some(split) = bytes.windows(4).position(|w| w == b"\r\n\r\n") else {
        return false;
    };
    let head = String::from_utf8_lossy(&bytes[..split]);
    let promised = head
        .lines()
        .find_map(|line| {
            let (name, value) = line.split_once(':')?;
            name.eq_ignore_ascii_case("content-length")
                .then(|| value.trim().parse::<usize>().ok())
                .flatten()
        })
        .unwrap_or(0);
    bytes.len() - (split + 4) >= promised
}

/// A client that ignores `HTTP_PROXY`/`HTTPS_PROXY`. The stub server below is
/// on localhost and must be reached directly: whatever proxy the developer's
/// shell exports is not in the business of forwarding to it. (The offline
/// tests in `projections.rs` used to set those variables process-wide, which
/// made these pass or fail depending on which test ran first; they now point
/// their own client at a dead host instead.)
pub(super) fn client() -> reqwest::Client {
    reqwest::Client::builder()
        .no_proxy()
        .build()
        .expect("http client")
}

pub(super) fn question() -> Vec<ChatMessage> {
    vec![ChatMessage {
        role: "user".into(),
        content: "Walker or Bowers?".into(),
    }]
}

/// The pause the wire tests retry with: long enough to be a pause, short
/// enough that three attempts are still a fast test. The budget beside it is
/// generous: the tests that care about it pass their own.
pub(super) const TEST_LIMITS: super::retry::Limits = super::retry::Limits {
    backoff: std::time::Duration::from_millis(10),
    pauses: std::time::Duration::from_secs(30),
};

pub(super) fn ask_url(url: &str, http: &reqwest::Client) -> Result<ChatReply, ChatError> {
    ask_url_with(url, http, CancelSignal::never())
}

/// The same, racing a cancel signal the test holds the other end of.
pub(super) fn ask_url_with(
    url: &str,
    http: &reqwest::Client,
    cancel: Arc<CancelSignal>,
) -> Result<ChatReply, ChatError> {
    ask_url_limited(url, http, cancel, TEST_LIMITS)
}

/// The same again, with the retry policy the test wants to show the edge of.
pub(super) fn ask_url_limited(
    url: &str,
    http: &reqwest::Client,
    cancel: Arc<CancelSignal>,
    limits: super::retry::Limits,
) -> Result<ChatReply, ChatError> {
    tokio_test_block(async {
        let call = Call {
            endpoint: url,
            http,
            api_key: "sk-ant-test",
            model: ChatModel::Opus5,
            effort: Effort::High,
            context: &context(),
            messages: &question(),
        };
        ask_at(call, cancel, limits, None).await
    })
}

/// The same again, watching the answer arrive: `progress` is handed the text
/// so far each time more of it lands.
pub(super) fn ask_url_watched(
    url: &str,
    http: &reqwest::Client,
    progress: super::OnProgress,
) -> Result<ChatReply, ChatError> {
    tokio_test_block(async {
        let call = Call {
            endpoint: url,
            http,
            api_key: "sk-ant-test",
            model: ChatModel::Opus5,
            effort: Effort::High,
            context: &context(),
            messages: &question(),
        };
        ask_at(call, CancelSignal::never(), TEST_LIMITS, Some(progress)).await
    })
}

pub(super) fn ask_stub(status: u16, body: impl Into<String>) -> Result<ChatReply, String> {
    ask_url(&stub_server(status, body.into()), &client()).map_err(String::from)
}
