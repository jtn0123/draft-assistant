//! Finding the address to put on screen, and a port to listen on.

use std::net::{SocketAddr, UdpSocket};

/// The first port tried, and how many after it.
pub const DEFAULT_PORT: u16 = 7878;
pub const PORT_ATTEMPTS: u16 = 10;

/// This machine's address on the LAN.
///
/// A UDP socket is *connected* to a public address and its local address read
/// back. Connecting a datagram socket sends nothing — it only asks the routing
/// table which interface would be used — so this works with no network
/// traffic, no name lookup, and no shelling out to `ifconfig`. When there is
/// no route at all (offline, or every interface down) the loopback address is
/// the honest answer: the server really is only reachable from this machine.
pub fn lan_ip() -> String {
    let Ok(socket) = UdpSocket::bind("0.0.0.0:0") else {
        return "127.0.0.1".to_string();
    };
    if socket.connect("8.8.8.8:80").is_err() {
        return "127.0.0.1".to_string();
    }
    match socket.local_addr() {
        Ok(SocketAddr::V4(addr)) => addr.ip().to_string(),
        _ => "127.0.0.1".to_string(),
    }
}

/// The URL the host shows and the QR code carries.
pub fn url_for(port: u16) -> String {
    format!("http://{}:{port}/", lan_ip())
}

/// This machine's Tailscale address, when it has one.
///
/// Tailscale hands every node an address in `100.64.0.0/10`, the carrier-grade
/// NAT range nothing on a home LAN uses, so an interface carrying one is the
/// tailnet. Read off `ifconfig` rather than a Tailscale CLI whose install
/// path varies; a phone on the tailnet can reach this address from anywhere.
pub fn tailscale_ip() -> Option<String> {
    let output = std::process::Command::new("ifconfig").output().ok()?;
    cgnat_address(&String::from_utf8_lossy(&output.stdout))
}

/// The first `100.64.0.0/10` address in `ifconfig` text.
pub fn cgnat_address(ifconfig: &str) -> Option<String> {
    ifconfig
        .lines()
        .filter_map(|line| {
            let mut words = line.split_whitespace();
            (words.next()? == "inet").then(|| words.next())?
        })
        .find(|ip| is_cgnat(ip))
        .map(str::to_string)
}

fn is_cgnat(ip: &str) -> bool {
    let mut parts = ip.split('.').map(|p| p.parse::<u8>().ok());
    matches!(
        (parts.next(), parts.next(), parts.next(), parts.next(), parts.next()),
        (Some(Some(100)), Some(Some(second)), Some(Some(_)), Some(Some(_)), None)
            if (64..=127).contains(&second)
    )
}

/// The tailnet URL for the same port, when this machine is on one.
pub fn tailscale_url_for(port: u16) -> Option<String> {
    tailscale_ip().map(|ip| format!("http://{ip}:{port}/"))
}

/// The two origins that are not this server and are still allowed to post to
/// it: the follower desktop's webview, and the Vite dev server it runs from
/// while the app is being worked on.
pub const FOLLOWER_ORIGINS: [&str; 2] = ["tauri://localhost", "http://localhost:1420"];

/// Whether a browser that says it is `origin` may make a state-changing
/// request of a server listening on `port`.
///
/// This is what stops a page on some other site the phone happens to have
/// open from posting to the companion in the background: a request from a
/// real origin has to be the phone page itself (same host and port as the
/// server, on an address a home network actually hands out) or one of the
/// follower origins. A request with no `Origin` at all is not a browser's
/// cross-site request and is left to the bearer token.
pub fn origin_allowed(origin: &str, port: Option<u16>) -> bool {
    if FOLLOWER_ORIGINS.contains(&origin) {
        return true;
    }
    let Some(port) = port else {
        return false;
    };
    let Some(rest) = origin
        .strip_prefix("http://")
        .or_else(|| origin.strip_prefix("https://"))
    else {
        return false;
    };
    let Some((host, said_port)) = rest.rsplit_once(':') else {
        return false;
    };
    said_port.parse::<u16>() == Ok(port) && is_local_address(host)
}

/// Whether a host is an address this server could be reached on: loopback, a
/// private LAN range, or the tailnet.
pub fn is_local_address(host: &str) -> bool {
    let host = host.trim_start_matches('[').trim_end_matches(']');
    if host == "localhost" {
        return true;
    }
    match host.parse::<std::net::IpAddr>() {
        Ok(std::net::IpAddr::V6(ip)) => ip.is_loopback(),
        Ok(std::net::IpAddr::V4(ip)) => {
            ip.is_loopback() || ip.is_private() || ip.is_link_local() || is_cgnat(&ip.to_string())
        }
        Err(_) => false,
    }
}

/// Bind `0.0.0.0` on the first free port from `first`, trying
/// [`PORT_ATTEMPTS`] of them. The port actually taken comes back with the
/// listener, because it is what the user has to type into their phone.
pub fn bind_from(first: u16) -> Result<(std::net::TcpListener, u16), String> {
    let mut last = String::new();
    for offset in 0..PORT_ATTEMPTS {
        let Some(port) = first.checked_add(offset) else {
            break;
        };
        match std::net::TcpListener::bind(("0.0.0.0", port)) {
            Ok(listener) => {
                // Non-blocking, because tokio's listener requires it.
                if let Err(e) = listener.set_nonblocking(true) {
                    return Err(format!("could not prepare the companion socket: {e}"));
                }
                // Read the port back off the socket rather than reporting the
                // one that was asked for: with `first` of 0 the kernel picks,
                // and the number shown to the user has to be the real one.
                let bound = listener
                    .local_addr()
                    .map(|addr| addr.port())
                    .unwrap_or(port);
                return Ok((listener, bound));
            }
            Err(e) => last = e.to_string(),
        }
    }
    Err(format!(
        "no free port between {first} and {} for the phone connection ({last})",
        first.saturating_add(PORT_ATTEMPTS - 1)
    ))
}

#[cfg(test)]
mod tests {
    use super::{bind_from, lan_ip, url_for, DEFAULT_PORT};

    #[test]
    fn an_address_is_always_produced_even_with_no_network() {
        let ip = lan_ip();
        assert!(!ip.is_empty());
        assert_eq!(ip.split('.').count(), 4, "{ip}");
        assert!(url_for(DEFAULT_PORT).starts_with("http://"));
        assert!(url_for(7879).ends_with(":7879/"));
    }

    #[test]
    fn only_a_carrier_grade_nat_address_counts_as_the_tailnet() {
        let text = "en0: flags=8863<UP>\n\tinet 192.168.1.242 netmask 0xffffff00\n\
                    utun4: flags=8051<UP>\n\tinet 100.101.102.103 --> 100.101.102.103 netmask 0xffffffff\n";
        assert_eq!(
            super::cgnat_address(text).as_deref(),
            Some("100.101.102.103")
        );
        assert_eq!(
            super::cgnat_address("inet 100.63.255.255\ninet 100.128.0.1\n"),
            None
        );
        assert_eq!(super::cgnat_address("inet 10.0.0.5\n"), None);
        assert_eq!(super::cgnat_address(""), None);
    }

    #[test]
    fn only_this_server_and_the_follower_may_post_across_origins() {
        use super::origin_allowed;
        // The phone page itself, however the phone reached the Mac.
        assert!(origin_allowed("http://192.168.1.42:7878", Some(7878)));
        assert!(origin_allowed("http://127.0.0.1:7878", Some(7878)));
        assert!(origin_allowed("http://100.101.102.103:7878", Some(7878)));
        // The follower desktop, which is its own origin and always will be.
        assert!(origin_allowed("tauri://localhost", None));
        assert!(origin_allowed("http://localhost:1420", None));
        // A page on the internet that found the port is not the phone page.
        assert!(!origin_allowed("https://evil.example.com", Some(7878)));
        assert!(!origin_allowed("http://evil.example.com:7878", Some(7878)));
        assert!(!origin_allowed("http://8.8.8.8:7878", Some(7878)));
        // Nor is the right address on somebody else's port.
        assert!(!origin_allowed("http://192.168.1.42:9999", Some(7878)));
        assert!(!origin_allowed("http://192.168.1.42", Some(7878)));
        assert!(!origin_allowed("null", Some(7878)));
    }

    #[test]
    fn a_busy_port_moves_the_server_along_to_the_next_one() {
        // Take a port, then ask to bind starting at it.
        let (held, port) = bind_from(0).expect("an ephemeral port is free");
        let (next, taken) = bind_from(port).expect("the next port is free");
        assert_ne!(taken, port, "the held port was handed out twice");
        assert!(taken > port && taken < port + super::PORT_ATTEMPTS);
        drop(next);
        drop(held);
    }
}
