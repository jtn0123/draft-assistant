//! When to ask the NFL what week it is.
//!
//! The season is loaded once, and everything after that — the poller, the
//! refresh command — reuses the week that load landed on. That is correct for
//! an app opened and closed inside an afternoon and wrong for one left running:
//! come Tuesday's rollover the screen was still scoring the week before, with
//! no way to notice short of restarting.
//!
//! Asking on every tick would be a request every thirty seconds for an answer
//! that changes once a week, so the check is rate limited here, where the rule
//! can be tested without a clock or a network.

/// How long between checks. The rollover is a Tuesday-morning event nobody is
/// watching to the second, and ten minutes is 1/20th of the poll traffic.
pub const CHECK_EVERY_SECS: u64 = 600;

/// Remembers when the week was last checked.
#[derive(Debug, Default)]
pub struct WeekWatch {
    checked_at: Option<u64>,
}

impl WeekWatch {
    /// True when it is time to ask again. The first call always is: the poller
    /// may have been started long after the load it inherited its week from.
    pub fn due(&self, now: u64) -> bool {
        match self.checked_at {
            None => true,
            Some(at) => now.saturating_sub(at) >= CHECK_EVERY_SECS,
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
        let mut watch = WeekWatch::default();
        assert!(watch.due(10_000), "a fresh poller has never asked");

        watch.checked(10_000);
        assert!(!watch.due(10_000));
        assert!(!watch.due(10_000 + CHECK_EVERY_SECS - 1));
        assert!(watch.due(10_000 + CHECK_EVERY_SECS));
    }

    #[test]
    fn a_clock_that_went_backwards_does_not_wrap_into_a_check_storm() {
        let mut watch = WeekWatch::default();
        watch.checked(10_000);
        assert!(
            !watch.due(9_000),
            "an earlier `now` must read as no time passed, not as an eternity"
        );
    }
}
