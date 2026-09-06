//! What this machine is called on its tailnet.
//!
//! The tailnet address alone works, but it is a number that can change when a
//! node is re-authenticated, and a QR code printed with a stale one sends the
//! phone nowhere. Tailscale also gives every node a MagicDNS name that stays
//! put, so the name is what goes on screen when the CLI can tell us one.

/// This machine as Tailscale sees it.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct TailscaleSelf {
    /// The MagicDNS name with no trailing dot, e.g. `justins-mac.tail1234.ts.net`.
    pub dns_name: Option<String>,
    /// The IPv4 address in `100.64.0.0/10`.
    pub ip: Option<String>,
}

impl TailscaleSelf {
    /// Only the address, which is what the `ifconfig` fallback can see.
    pub fn from_ip(ip: String) -> Self {
        Self {
            dns_name: None,
            ip: Some(ip),
        }
    }

    /// Nothing known at all, which is the same as not being on a tailnet.
    pub fn is_empty(&self) -> bool {
        self.dns_name.is_none() && self.ip.is_none()
    }

    /// Every host a phone might have typed or scanned, name first.
    ///
    /// Both are listed because the two are the same machine: a phone that
    /// scanned the name and a phone that was handed the number have to pass
    /// the same cross-origin check.
    pub fn hosts(&self) -> impl Iterator<Item = &str> {
        self.dns_name
            .as_deref()
            .into_iter()
            .chain(self.ip.as_deref())
    }

    /// The host to put on screen: the name when there is one.
    pub fn preferred_host(&self) -> Option<&str> {
        self.hosts().next()
    }
}

/// Where the Tailscale CLI is, in the order worth trying.
///
/// The bare name covers a `PATH` that already has it; the app bundle is where
/// the Mac App Store build puts it, and `/usr/local/bin` is the symlink the
/// standalone installer makes. Shared with the certificate minting in
/// [`super::tls`], which has to find the same binary.
pub const CLI_PATHS: [&str; 3] = [
    "tailscale",
    "/Applications/Tailscale.app/Contents/MacOS/Tailscale",
    "/usr/local/bin/tailscale",
];

/// Ask the Tailscale CLI who this machine is.
///
/// Returns `None` when no CLI is installed or the backend is not running, so
/// the caller can fall back to reading an address off `ifconfig`. This spawns
/// a process, so it belongs on the slow origin-refresh timer and never in the
/// path of a request.
pub fn tailscale_status() -> Option<TailscaleSelf> {
    for path in CLI_PATHS {
        let Ok(output) = std::process::Command::new(path)
            .args(["status", "--json"])
            .output()
        else {
            continue;
        };
        if !output.status.success() {
            continue;
        }
        if let Some(found) = parse_status(&String::from_utf8_lossy(&output.stdout)) {
            return Some(found);
        }
    }
    None
}

/// The name and address out of `tailscale status --json`.
///
/// `None` when the backend is not `Running` — a logged-out or stopped
/// Tailscale still prints a `Self` block with the last address it had, and
/// putting that on a QR code would advertise a route that does not exist.
pub fn parse_status(json: &str) -> Option<TailscaleSelf> {
    let value: serde_json::Value = serde_json::from_str(json).ok()?;
    if value.get("BackendState").and_then(|s| s.as_str()) != Some("Running") {
        return None;
    }
    let this = value.get("Self")?;
    let dns_name = this
        .get("DNSName")
        .and_then(|n| n.as_str())
        .map(|n| n.trim_end_matches('.'))
        .filter(|n| !n.is_empty())
        .map(str::to_string);
    // The list carries the IPv6 address too; only the v4 one goes in a URL
    // without brackets, and it is the one people recognise.
    let ip = this
        .get("TailscaleIPs")
        .and_then(|ips| ips.as_array())
        .and_then(|ips| {
            ips.iter()
                .filter_map(|ip| ip.as_str())
                .find(|ip| ip.parse::<std::net::Ipv4Addr>().is_ok())
        })
        .map(str::to_string);
    let found = TailscaleSelf { dns_name, ip };
    (!found.is_empty()).then_some(found)
}

#[cfg(test)]
mod tests {
    use super::{parse_status, TailscaleSelf};

    // Hand-written, not captured from a real tailnet: it carries only the two
    // fields this code reads, in the shape `tailscale status --json` prints.
    const RUNNING: &str = r#"{
        "BackendState": "Running",
        "Self": {
            "DNSName": "justins-mac.tail1234.ts.net.",
            "TailscaleIPs": ["100.101.102.103", "fd7a:115c:a1e0::1"]
        }
    }"#;

    #[test]
    fn the_magic_dns_name_loses_its_trailing_dot_and_the_v6_address_is_skipped() {
        let found = parse_status(RUNNING).expect("a running backend");
        assert_eq!(
            found.dns_name.as_deref(),
            Some("justins-mac.tail1234.ts.net")
        );
        assert_eq!(found.ip.as_deref(), Some("100.101.102.103"));
        assert_eq!(found.preferred_host(), Some("justins-mac.tail1234.ts.net"));
        assert_eq!(
            found.hosts().collect::<Vec<_>>(),
            vec!["justins-mac.tail1234.ts.net", "100.101.102.103"]
        );
    }

    #[test]
    fn a_stopped_or_unreadable_tailscale_is_no_tailnet_at_all() {
        // The failure this prevents: a logged-out node still prints the
        // address it used to have, and that address routes nowhere.
        let stopped = RUNNING.replace("Running", "Stopped");
        assert_eq!(parse_status(&stopped), None);
        assert_eq!(parse_status("{}"), None);
        assert_eq!(parse_status("not json at all"), None);
        assert_eq!(
            parse_status(r#"{"BackendState":"Running","Self":{"DNSName":"","TailscaleIPs":[]}}"#),
            None
        );
    }

    #[test]
    fn an_address_with_no_name_is_still_a_machine() {
        let only_ip =
            parse_status(r#"{"BackendState":"Running","Self":{"TailscaleIPs":["100.64.0.7"]}}"#)
                .expect("an address is enough");
        assert_eq!(only_ip, TailscaleSelf::from_ip("100.64.0.7".to_string()));
        assert_eq!(only_ip.preferred_host(), Some("100.64.0.7"));
    }
}
