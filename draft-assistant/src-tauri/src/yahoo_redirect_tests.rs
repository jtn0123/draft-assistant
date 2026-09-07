//! The loopback listener's own limits. The redirect parser and the happy
//! path are next door in `yahoo_oauth_tests.rs`; what is here is the ceiling
//! on how much of one request the listener will hold.

// The io traits, the socket types and the clock all arrive with the parent.
use super::*;

#[test]
fn a_request_that_never_ends_is_dropped_rather_than_buffered() {
    // The failure this prevents: the buffer grew until a blank line arrived
    // or ten seconds passed, so any local process could make the listener
    // allocate as fast as it could write, and the real redirect queued behind
    // it waited out the whole read window.
    let listener = TcpListener::bind("127.0.0.1:0").expect("bind");
    let port = listener.local_addr().expect("addr").port();
    let caught =
        std::thread::spawn(move || catch_redirect_on_within(listener, Duration::from_secs(30)));

    // A connection that writes past the ceiling and never sends a blank line.
    let mut flood = TcpStream::connect(("127.0.0.1", port)).expect("connect");
    flood
        .write_all(b"GET /?code=x&state=")
        .expect("the request line");
    let filler = vec![b'a'; REDIRECT_MAX_REQUEST + 4_096];
    // A closed pipe here is the listener having let go, which is the point.
    let _ = flood.write_all(&filler);
    let _ = flood.flush();

    // The listener is free again long before the ten-second read window it
    // would otherwise have spent on that socket.
    let started = Instant::now();
    let mut socket = TcpStream::connect(("127.0.0.1", port)).expect("connect");
    socket
        .write_all(b"GET /?code=live-code&state=nonce-1 HTTP/1.1\r\nHost: localhost\r\n\r\n")
        .expect("write");
    let mut page = String::new();
    let _ = socket.read_to_string(&mut page);
    let redirect = caught
        .join()
        .expect("listener thread")
        .expect("the redirect arrived behind the flood");
    assert_eq!(redirect.code, "live-code");
    assert_eq!(redirect.state, "nonce-1");
    assert!(
        started.elapsed() < REDIRECT_READ_WAIT / 2,
        "the flood held the listener for {:?}",
        started.elapsed()
    );
    drop(flood);
}

#[test]
fn the_ceiling_leaves_room_for_a_real_redirect() {
    // Yahoo's code and the state are together a couple of hundred bytes; the
    // ceiling has to be well clear of anything a browser would really send.
    const { assert!(REDIRECT_MAX_REQUEST >= 4_096) };
    let target = format!("/?code={}&state={}", "c".repeat(256), "s".repeat(64));
    let request = format!("GET {target} HTTP/1.1\r\nHost: localhost\r\n\r\n");
    assert!(request.len() < REDIRECT_MAX_REQUEST, "{}", request.len());
    assert_eq!(parse_redirect(&target).code, "c".repeat(256));
}
