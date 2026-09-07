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

/// Five wrong codes from one address inside this window lock that address
/// out; twenty from everyone at once lock every address out for the same
/// minute.
const PAIR_WINDOW_MS: u64 = 60_000;
const PAIR_MAX_FAILURES: usize = 5;
const PAIR_LOCKOUT_MS: u64 = 60_000;
/// Wrong codes from all addresses together before pairing closes for a minute.
///
/// The per-address count alone was the whole of the limit, so somebody on the
/// same network who could present several source addresses had five guesses a
/// minute *each* against the six digits that gate the read API and the host's
/// chat budget. Four times the per-address allowance, so a household where a
/// few people mistype the code at once is not shut out by each other.
const PAIR_MAX_FAILURES_ALL: usize = 20;

/// Wrong codes, counted per address and again across all of them.
///
/// The per-address count is keyed by peer so one guesser on the network cannot
/// lock the phone in the owner's hand out of its own house. The count across
/// every address is what stops that same guesser simply changing address.
#[derive(Default)]
pub struct Lockout {
    failures: HashMap<IpAddr, Vec<u64>>,
    locked_until_ms: HashMap<IpAddr, u64>,
    /// Every recent wrong code, whoever sent it.
    all_failures: Vec<u64>,
    /// While this is in the future, no address may try at all.
    all_locked_until_ms: u64,
}

impl Lockout {
    /// Whether this address has spent its guesses and must wait, either on
    /// its own account or because pairing is shut for everyone.
    pub fn locked(&self, peer: IpAddr, now: u64) -> bool {
        now < self.all_locked_until_ms
            || now < self.locked_until_ms.get(&peer).copied().unwrap_or(0)
    }

    /// Count one wrong code against the address it came from and against the
    /// network as a whole, and lock out whichever has spent its allowance.
    pub fn note_failure(&mut self, peer: IpAddr, now: u64) {
        self.prune(now);
        self.all_failures.push(now);
        if self.all_failures.len() >= PAIR_MAX_FAILURES_ALL {
            self.all_failures.clear();
            self.all_locked_until_ms = now + PAIR_LOCKOUT_MS;
        }
        let recent = self.failures.entry(peer).or_default();
        recent.push(now);
        if recent.len() >= PAIR_MAX_FAILURES {
            recent.clear();
            self.locked_until_ms.insert(peer, now + PAIR_LOCKOUT_MS);
        }
        self.failures.retain(|_, at| !at.is_empty());
    }

    /// Forget every address whose window and lockout have both passed. Only
    /// the guessing address used to be trimmed, so a scan across the LAN left
    /// one entry per address behind for the life of the app.
    fn prune(&mut self, now: u64) {
        self.all_failures
            .retain(|at| now.saturating_sub(*at) < PAIR_WINDOW_MS);
        if now >= self.all_locked_until_ms {
            self.all_locked_until_ms = 0;
        }
        self.locked_until_ms.retain(|_, until| now < *until);
        self.failures.retain(|_, at| {
            at.retain(|at| now.saturating_sub(*at) < PAIR_WINDOW_MS);
            !at.is_empty()
        });
    }

    /// How many addresses are being remembered, for the test that says the
    /// maps do not grow for ever.
    pub fn tracked(&self) -> usize {
        let mut peers: Vec<&IpAddr> = self.failures.keys().collect();
        peers.extend(self.locked_until_ms.keys());
        peers.sort_unstable();
        peers.dedup();
        peers.len()
    }

    /// A code that worked wipes the slate for that address.
    ///
    /// Only for that address. Somebody pairing successfully says nothing about
    /// whoever else was guessing, and the shared count clears itself a minute
    /// after the last wrong code either way.
    pub fn forgive(&mut self, peer: IpAddr) {
        self.failures.remove(&peer);
        self.locked_until_ms.remove(&peer);
    }

    pub fn clear(&mut self) {
        self.failures.clear();
        self.locked_until_ms.clear();
        self.all_failures.clear();
        self.all_locked_until_ms = 0;
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

    /// The failure this prevents: the lockout was keyed per address and
    /// nothing else, so somebody on the same network presenting a fresh
    /// source address for every five guesses had no limit at all against the
    /// six digits that gate the whole read API.
    #[test]
    fn guesses_spread_over_many_addresses_still_run_out() {
        let mut lockout = Lockout::default();
        // Four addresses, four wrong codes each: nobody has spent their own
        // five, and sixteen wrong codes in a minute is still under the cap.
        for last in 0..4u8 {
            for _ in 0..4 {
                lockout.note_failure(peer(last), 1_000);
            }
        }
        assert!(!lockout.locked(peer(99), 1_000), "under the shared cap");
        for _ in 0..4 {
            lockout.note_failure(peer(4), 1_000);
        }
        // The twentieth wrong code shuts pairing for every address, including
        // one that has never guessed.
        assert!(lockout.locked(peer(99), 1_000));
        assert!(lockout.locked(peer(0), 1_000));
        // And for a minute only.
        assert!(!lockout.locked(peer(99), 1_000 + 60_001));
    }

    #[test]
    fn wrong_codes_from_everyone_spread_over_more_than_a_minute_lock_nobody_out() {
        let mut lockout = Lockout::default();
        for n in 0..40u64 {
            lockout.note_failure(peer(n as u8 % 8), n * 10_000);
        }
        assert!(!lockout.locked(peer(99), 400_000));
    }

    /// One person pairing does not vouch for whoever else was guessing.
    #[test]
    fn a_successful_pairing_does_not_reopen_a_network_wide_lockout() {
        let mut lockout = Lockout::default();
        for last in 0..20u8 {
            lockout.note_failure(peer(last), 1_000);
        }
        assert!(lockout.locked(peer(7), 1_000));
        lockout.forgive(peer(7));
        assert!(lockout.locked(peer(7), 1_000));
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

    #[test]
    fn addresses_whose_minute_has_passed_are_forgotten_rather_than_kept_for_ever() {
        let mut lockout = Lockout::default();
        // A scan: one wrong code from each of two hundred addresses, and one
        // of them locked out. Nothing here is worth remembering a minute on.
        for last in 0..200u8 {
            lockout.note_failure(peer(last), 1_000);
        }
        for _ in 0..5 {
            lockout.note_failure(peer(250), 1_000);
        }
        assert!(lockout.locked(peer(250), 1_000));
        assert_eq!(lockout.tracked(), 201);
        // The next wrong code from anyone, after the window and the lockout
        // have both passed, is the only one left on the books.
        lockout.note_failure(peer(251), 1_000 + 60_001);
        assert_eq!(lockout.tracked(), 1);
        assert!(!lockout.locked(peer(250), 1_000 + 60_001));
    }
}
