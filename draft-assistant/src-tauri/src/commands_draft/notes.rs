//! Deciding which of a poll tick's notes are worth a log line, and when a
//! slow tick is.
//!
//! Both follow `applog::HealthWatch`: say it when it starts, say it when it
//! stops, and say nothing in between. A note is a problem that is not a
//! failed tick (a `/draft` that did not answer, a keeper file that did not
//! write), and the loop used to log every one of them on every tick, three
//! seconds apart, for as long as it lasted: one sulking endpoint filled the
//! log with the same sentence a thousand times an hour and rotated the
//! interesting part away.

use std::collections::BTreeSet;
use std::time::Duration;

/// The notes still standing as of the last tick, so a repeat can be told
/// from a new one and a clearing from a quiet tick.
#[derive(Debug, Default)]
pub(super) struct NoteWatch {
    standing: BTreeSet<String>,
}

impl NoteWatch {
    /// The lines to log for this tick's notes: each note the first tick it
    /// appears, and each standing note the first tick it is gone.
    pub(super) fn observe(&mut self, notes: &[String]) -> Vec<String> {
        let now: BTreeSet<String> = notes.iter().cloned().collect();
        let mut lines: Vec<String> = now.difference(&self.standing).cloned().collect();
        lines.extend(
            self.standing
                .difference(&now)
                .map(|note| format!("cleared: {note}")),
        );
        self.standing = now;
        lines
    }
}

/// Whether the last tick was reported as slow, so the report is made once
/// per stretch rather than once per tick.
#[derive(Debug, Default)]
pub(super) struct SlowTickWatch {
    slow: bool,
}

impl SlowTickWatch {
    /// What to log about a tick that took `took` against a poll interval of
    /// `interval`, or `None` when nothing changed.
    ///
    /// A tick longer than its interval means the next one starts late, and
    /// the picks on screen are older than the badge says. That used to be
    /// invisible: the badge reads success or failure, never how long either
    /// took.
    pub(super) fn observe(&mut self, took: Duration, interval: Duration) -> Option<String> {
        let slow = took > interval;
        if slow && !self.slow {
            self.slow = true;
            return Some(format!(
                "poll tick took {:.1}s, longer than the {}s interval: the picks on screen are older than the sync badge says",
                took.as_secs_f64(),
                interval.as_secs()
            ));
        }
        if !slow && self.slow {
            self.slow = false;
            return Some("poll tick back inside its interval".to_string());
        }
        None
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn notes(list: &[&str]) -> Vec<String> {
        list.iter().map(|note| (*note).to_string()).collect()
    }

    /// A `/draft` that stopped answering wrote the same warning every three
    /// seconds for as long as it sulked.
    #[test]
    fn a_note_is_logged_when_it_appears_and_when_it_clears_and_not_in_between() {
        let mut watch = NoteWatch::default();
        let first = watch.observe(&notes(&["draft status not refreshed: 500"]));
        assert_eq!(first, notes(&["draft status not refreshed: 500"]));
        for _ in 0..100 {
            assert!(
                watch
                    .observe(&notes(&["draft status not refreshed: 500"]))
                    .is_empty(),
                "the same note again is not news"
            );
        }
        assert_eq!(
            watch.observe(&[]),
            notes(&["cleared: draft status not refreshed: 500"])
        );
        assert!(
            watch.observe(&[]).is_empty(),
            "a quiet tick after a quiet tick says nothing"
        );
    }

    #[test]
    fn a_second_note_beside_a_standing_one_is_reported_on_its_own() {
        let mut watch = NoteWatch::default();
        watch.observe(&notes(&["a"]));
        assert_eq!(watch.observe(&notes(&["a", "b"])), notes(&["b"]));
        // `a` clears while `b` stands.
        assert_eq!(watch.observe(&notes(&["b"])), notes(&["cleared: a"]));
        // Back again after clearing is news again.
        assert_eq!(watch.observe(&notes(&["a", "b"])), notes(&["a"]));
    }

    /// A tick that ran 25 seconds under a green badge left no trace at all.
    #[test]
    fn a_slow_tick_is_reported_once_and_its_recovery_once() {
        let mut watch = SlowTickWatch::default();
        let interval = Duration::from_secs(3);
        assert_eq!(watch.observe(Duration::from_millis(400), interval), None);
        let slow = watch
            .observe(Duration::from_secs(25), interval)
            .expect("the first slow tick is reported");
        assert!(
            slow.contains("25.0s") && slow.contains("3s interval"),
            "{slow}"
        );
        assert_eq!(watch.observe(Duration::from_secs(26), interval), None);
        assert_eq!(
            watch
                .observe(Duration::from_millis(300), interval)
                .as_deref(),
            Some("poll tick back inside its interval")
        );
        assert_eq!(watch.observe(Duration::from_millis(300), interval), None);
    }
}
