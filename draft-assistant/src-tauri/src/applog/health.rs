//! Deciding when a poller's health is worth a log line.
//!
//! The failure this exists to prevent is both halves of the obvious mistake.
//! Logging every tick fills a megabyte in an hour and rotates the interesting
//! part of the log away; logging nothing -- which is what the pollers did --
//! leaves "it stopped updating around 8:40" with no answer at all.
//!
//! So a line is written only when something moved: the poller started failing,
//! it recovered, or the backoff between tries changed while it was failing.
//! Three lines for an hour of trouble, and the first of them says why.

/// One poller's remembered health, so a transition can be told from a repeat.
#[derive(Debug, Default)]
pub struct HealthWatch {
    failing: bool,
    /// The gap between tries as of the last line written. Only meaningful
    /// while `failing`, but kept either way so a recovery does not report a
    /// backoff change on the way back to normal.
    backoff: u64,
}

impl HealthWatch {
    /// What to log about this tick, or `None` when nothing changed.
    ///
    /// `failures` is the consecutive-failure count the tick just recorded,
    /// `wait` the seconds until the next try, and `error` the reason the last
    /// failure gave.
    pub fn observe(&mut self, failures: u32, wait: u64, error: Option<&str>) -> Option<String> {
        let reason = error.unwrap_or("no reason given");
        if failures > 0 && !self.failing {
            self.failing = true;
            self.backoff = wait;
            return Some(format!(
                "poll started failing after {failures} tries, next try in {wait}s: {reason}"
            ));
        }
        if failures == 0 && self.failing {
            self.failing = false;
            self.backoff = wait;
            return Some("poll recovered".to_string());
        }
        if self.failing && wait != self.backoff {
            self.backoff = wait;
            return Some(format!(
                "poll still failing after {failures} tries, next try in {wait}s: {reason}"
            ));
        }
        self.backoff = wait;
        None
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_first_failure_is_reported_and_the_identical_ones_after_it_are_not() {
        let mut watch = HealthWatch::default();
        let first = watch.observe(1, 3, Some("sleeper timed out"));
        assert!(first.is_some_and(|line| line.contains("sleeper timed out")));
        // The same failure, at the same backoff, three seconds later. A line
        // per tick here is what buries the log.
        assert_eq!(watch.observe(2, 3, Some("sleeper timed out")), None);
        assert_eq!(watch.observe(3, 3, Some("sleeper timed out")), None);
    }

    #[test]
    fn a_stretching_backoff_is_reported_once_per_step() {
        let mut watch = HealthWatch::default();
        watch.observe(1, 3, Some("timed out"));
        let stretched = watch.observe(4, 12, Some("timed out"));
        assert!(stretched.is_some_and(|line| line.contains("12s")));
        assert_eq!(watch.observe(5, 12, Some("timed out")), None);
    }

    #[test]
    fn recovery_is_reported_once_and_the_next_healthy_tick_is_silent() {
        let mut watch = HealthWatch::default();
        watch.observe(1, 3, Some("timed out"));
        assert_eq!(watch.observe(0, 3, None).as_deref(), Some("poll recovered"));
        assert_eq!(watch.observe(0, 3, None), None);
    }

    #[test]
    fn a_poller_that_never_fails_writes_nothing_at_all() {
        let mut watch = HealthWatch::default();
        for _ in 0..100 {
            assert_eq!(watch.observe(0, 3, None), None);
        }
    }

    #[test]
    fn a_failure_with_no_reason_still_says_something_readable() {
        let mut watch = HealthWatch::default();
        let line = watch
            .observe(1, 3, None)
            .expect("the first failure is logged");
        assert!(line.contains("no reason given"), "{line}");
    }

    #[test]
    fn failing_again_after_a_recovery_is_reported_afresh() {
        let mut watch = HealthWatch::default();
        watch.observe(1, 3, Some("first"));
        watch.observe(0, 3, None);
        let again = watch.observe(1, 3, Some("second"));
        assert!(again.is_some_and(|line| line.contains("second")));
    }
}
