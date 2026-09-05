//! The parts of pairing that are not the hub's own bookkeeping: the device a
//! client is handed back, the shape of one attempt, and the per-address
//! lockout that is what makes a six digit code worth typing at all.
//!
//! Split out of `hub.rs` so that file stays under the repository's size cap;
//! nothing in here reaches back into the hub.

use serde::{Deserialize, Serialize};
use std::collections::HashMap;
use std::net::IpAddr;

/// A paired phone or follower desktop, as the contract describes it.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Device {
    pub device_id: String,
    pub name: String,
    /// "phone" or "desktop".
    pub kind: String,
    pub paired_at_ms: u64,
    pub last_seen_ms: u64,
    pub connected: bool,
}

/// One paired device plus the secret nobody outside the companion sees.
#[derive(Debug, Clone)]
pub struct Paired {
    pub token: String,
    pub device: Device,
    /// Open WebSockets for this device. `connected` is this being non-zero.
    pub sockets: u32,
    /// When this device posted its recent chat questions, for the per-minute cap.
    pub posts: Vec<u64>,
}

/// One attempt to pair, as the route hands it over.
pub struct PairAttempt<'a> {
    pub code: &'a str,
    pub name: &'a str,
    pub kind: &'a str,
    /// The address the attempt came from; the lockout is counted per address.
    pub peer: IpAddr,
    /// The id this client was given last time, when it has one. Only a client
    /// that proves it is the same device replaces its old entry; anyone else
    /// pairing under the same name gets a name of its own.
    pub previous_device_id: Option<&'a str>,
}

/// The outcome of an attempt to pair.
pub enum PairOutcome {
    Ok {
        token: String,
        device_id: String,
        host_name: String,
    },
    WrongCode,
    LockedOut,
}

/// Five wrong codes inside this window locks that one address out.
const PAIR_WINDOW_MS: u64 = 60_000;
const PAIR_MAX_FAILURES: usize = 5;
const PAIR_LOCKOUT_MS: u64 = 60_000;

/// Wrong codes, counted per address.
///
/// Keyed by peer so one guesser on the network cannot lock the phone in the
/// owner's hand out of its own house.
#[derive(Default)]
pub struct Lockout {
    failures: HashMap<IpAddr, Vec<u64>>,
    locked_until_ms: HashMap<IpAddr, u64>,
}

impl Lockout {
    /// Whether this address has spent its guesses and must wait.
    pub fn locked(&self, peer: IpAddr, now: u64) -> bool {
        now < self.locked_until_ms.get(&peer).copied().unwrap_or(0)
    }

    /// Count one wrong code against the address it came from, and lock that
    /// address out once it has spent five inside the window.
    pub fn note_failure(&mut self, peer: IpAddr, now: u64) {
        let recent = self.failures.entry(peer).or_default();
        recent.retain(|at| now.saturating_sub(*at) < PAIR_WINDOW_MS);
        recent.push(now);
        if recent.len() >= PAIR_MAX_FAILURES {
            recent.clear();
            self.locked_until_ms.insert(peer, now + PAIR_LOCKOUT_MS);
        }
    }

    /// A code that worked wipes the slate for that address.
    pub fn forgive(&mut self, peer: IpAddr) {
        self.failures.remove(&peer);
        self.locked_until_ms.remove(&peer);
    }

    pub fn clear(&mut self) {
        self.failures.clear();
        self.locked_until_ms.clear();
    }
}

#[cfg(test)]
mod tests {
    use super::Lockout;
    use std::net::IpAddr;

    fn peer(last: u8) -> IpAddr {
        IpAddr::from([192, 168, 1, last])
    }

    #[test]
    fn five_wrong_codes_lock_one_address_and_leave_the_others_alone() {
        let mut lockout = Lockout::default();
        for _ in 0..5 {
            lockout.note_failure(peer(66), 1_000);
        }
        assert!(lockout.locked(peer(66), 1_000));
        assert!(!lockout.locked(peer(11), 1_000));
        // The minute passes and the guesser may try again.
        assert!(!lockout.locked(peer(66), 1_000 + 60_001));
    }

    #[test]
    fn wrong_codes_spread_over_more_than_a_minute_do_not_lock_anyone_out() {
        let mut lockout = Lockout::default();
        for n in 0..5 {
            lockout.note_failure(peer(66), n * 30_000);
        }
        assert!(!lockout.locked(peer(66), 120_000));
    }

    #[test]
    fn a_code_that_worked_wipes_the_slate() {
        let mut lockout = Lockout::default();
        for _ in 0..5 {
            lockout.note_failure(peer(66), 1_000);
        }
        lockout.forgive(peer(66));
        assert!(!lockout.locked(peer(66), 1_000));
    }
}
