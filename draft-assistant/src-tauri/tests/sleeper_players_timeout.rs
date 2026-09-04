//! The players dictionary gets a deadline of its own.
//!
//! The client-wide timeout is eight seconds, which is right for the small
//! JSON endpoints and hopeless for `/players/nfl`: ~14.6 MB needs about
//! 15 Mbps to arrive inside eight seconds, so on a hotspot or a crowded
//! draft-night wifi the request was cancelled every single time and a cold
//! start could never finish. This serves that route slower than the
//! client-wide timeout and checks it still lands.

use draft_assistant_lib::sleeper::SleeperClient;
use std::io::{Read, Write};
use std::net::TcpListener;
use std::time::Duration;

/// Longer than the client-wide timeout, comfortably shorter than the
/// players-only one.
const DELAY: Duration = Duration::from_secs(9);

/// A stub that answers every request `DELAY` late, the way a slow link makes
/// a big body arrive late.
fn slow_server() -> String {
    let listener = TcpListener::bind("127.0.0.1:0").expect("bind");
    let addr = listener.local_addr().expect("addr");
    std::thread::spawn(move || {
        for stream in listener.incoming().flatten() {
            std::thread::spawn(move || {
                let mut socket = stream;
                let mut request = Vec::new();
                let mut chunk = [0u8; 8192];
                while !request.windows(4).any(|w| w == b"\r\n\r\n") {
                    match socket.read(&mut chunk) {
                        Ok(0) | Err(_) => return,
                        Ok(n) => request.extend_from_slice(&chunk[..n]),
                    }
                }
                std::thread::sleep(DELAY);
                let body = "{\"4034\":{\"position\":\"RB\"}}";
                let response = format!(
                    "HTTP/1.1 200 OK\r\nContent-Type: application/json\r\nContent-Length: {}\r\nConnection: close\r\n\r\n{body}",
                    body.len()
                );
                let _ = socket.write_all(response.as_bytes());
                let _ = socket.flush();
                let _ = socket.shutdown(std::net::Shutdown::Write);
            });
        }
    });
    format!("http://{addr}")
}

#[tokio::test(flavor = "multi_thread")]
async fn the_players_dictionary_outlives_the_client_wide_timeout() {
    let client = SleeperClient::with_host(slow_server());
    let started = std::time::Instant::now();

    // Both requests go to the same slow stub at once, so the whole test costs
    // one delay rather than two.
    let (players, league) = tokio::join!(
        client.players_bytes(),
        // Everything else keeps the eight-second deadline. Given eleven
        // seconds of rope, a request that kept the small-endpoint timeout is
        // still in its retry loop and has nothing to show; one that had
        // quietly been given the players timeout would have answered at nine.
        tokio::time::timeout(Duration::from_secs(11), client.league("123")),
    );

    let bytes = players.expect("the players dictionary should survive a slow link");
    assert!(
        bytes.starts_with(b"{\"4034\""),
        "unexpected body: {}",
        String::from_utf8_lossy(&bytes)
    );
    assert!(
        started.elapsed() >= DELAY,
        "the stub was supposed to be slower than the client-wide timeout"
    );
    assert!(
        league.is_err(),
        "a small endpoint must keep the short deadline"
    );
}
