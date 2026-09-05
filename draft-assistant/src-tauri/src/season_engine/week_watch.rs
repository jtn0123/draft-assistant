//! When to ask the NFL what week it is, and when to re-read the player
//! dictionary.
//!
//! The season is loaded once, and everything after that — the poller, the
//! refresh command — reuses what that load landed on. That is correct for an
//! app opened and closed inside an afternoon and wrong for one left running:
//! come Tuesday's rollover the screen was still scoring the week before, and
//! the injury tags were still whatever they were when the league was opened,
//! with no way to notice short of restarting.
//!
//! Asking on every tick would be a request every thirty seconds for an answer
//! that changes once a week, so both checks are rate limited here, where the
//! rule can be tested without a clock or a network.

/// How long between week checks. The rollover is a Tuesday-morning event
/// nobody is watching to the second, and ten minutes is 1/20th of the poll
/// traffic.
pub const CHECK_EVERY_SECS: u64 = 600;

/// How long between player-dictionary refreshes. Injury statuses move on a
/// news cycle, not on a play clock, and the dictionary is ~14.6 MB — half an
/// hour is often enough to catch a Sunday-morning downgrade and rare enough
/// that it costs nothing.
pub const PLAYERS_EVERY_SECS: u64 = 1800;

/// Remembers when something was last checked, and refuses to check again
/// until its interval has passed.
#[derive(Debug)]
pub struct Watch {
    every: u64,
    checked_at: Option<u64>,
}

impl Watch {
    /// A watch that fires at most once per `every` seconds.
    pub const fn every(every: u64) -> Self {
        Self {
            every,
            checked_at: None,
        }
    }

    /// True when it is time to ask again. The first call always is: the poller
    /// may have been started long after the load it inherited its data from.
    pub fn due(&self, now: u64) -> bool {
        match self.checked_at {
            None => true,
            Some(at) => now.saturating_sub(at) >= self.every,
        }
    }

    /// Note that a check just happened, however it turned out.
    ///
    /// A failed check counts. Otherwise an outage turns the rate limit off and
    /// every tick fires another doomed request at an endpoint that is already
    /// down.
    pub fn checked(&mut self, now: u64) {
        self.checked_at = Some(now);
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_first_check_happens_immediately_and_the_next_one_waits() {
        let mut watch = Watch::every(CHECK_EVERY_SECS);
        assert!(watch.due(10_000), "a fresh poller has never asked");

        watch.checked(10_000);
        assert!(!watch.due(10_000));
        assert!(!watch.due(10_000 + CHECK_EVERY_SECS - 1));
        assert!(watch.due(10_000 + CHECK_EVERY_SECS));
    }

    #[test]
    fn a_clock_that_went_backwards_does_not_wrap_into_a_check_storm() {
        let mut watch = Watch::every(CHECK_EVERY_SECS);
        watch.checked(10_000);
        assert!(
            !watch.due(9_000),
            "an earlier `now` must read as no time passed, not as an eternity"
        );
    }

    /// The two clocks are independent: the injury refresh is three times as
    /// slow as the week check, and one firing must not reset the other.
    #[test]
    fn each_watch_keeps_its_own_interval() {
        let mut players = Watch::every(PLAYERS_EVERY_SECS);
        players.checked(10_000);
        assert!(!players.due(10_000 + CHECK_EVERY_SECS));
        assert!(players.due(10_000 + PLAYERS_EVERY_SECS));
    }
}
