//! What the season loop says about a source that keeps failing and about a
//! tick that runs long, and when it says it.
//!
//! The draft loop has had both since it was built (`commands_draft::notes`);
//! this is the same rule on the other loop: a line when it starts, a line
//! when it stops, nothing in between.

use crate::commands_draft::tick::backoff_secs;
use crate::season_sources::{SourceHealth, SourceStatus};
use std::collections::BTreeMap;
use std::time::Duration;

/// How long one source may fail before it is worth a line of its own.
///
/// The poll health badge only knows about a tick where *everything* failed:
/// a partial success is a success, which is right for the screen (two feeds
/// arriving is most of the screen) and wrong for the log, where one feed
/// down for an hour read as an hour of green. Ten ticks at the default
/// thirty seconds.
pub(super) const SOURCE_NOTE_AFTER_SECS: u64 = 300;

/// Which sources were last reported failing, and with what reason.
#[derive(Debug, Default)]
pub(super) struct SourceWatch {
    standing: BTreeMap<&'static str, String>,
}

impl SourceWatch {
    /// The lines to log for this tick's per-source health: each source the
    /// first tick it has been failing long enough (or its reason changes),
    /// and each standing one the first tick it is back. `None` is a tick
    /// with no season loaded, which clears the slate in silence.
    pub(super) fn observe(&mut self, sources: Option<&SourceHealth>, now: u64) -> Vec<String> {
        let Some(sources) = sources else {
            self.standing.clear();
            return Vec::new();
        };
        let mut lines = Vec::new();
        let mut still: BTreeMap<&'static str, String> = BTreeMap::new();
        let named: Vec<(&'static str, &SourceStatus)> = [
            ("matchups", &sources.matchups),
            ("scores", &sources.scores),
            ("rosters", &sources.rosters),
        ]
        .into_iter()
        // The dictionary joins the list once the half-hour refresh has run
        // for the first time; before that there is nothing to report.
        .chain(sources.players.as_ref().map(|s| ("players", s)))
        .collect();
        for (name, status) in named {
            let Some(reason) = failing_long_enough(status, now) else {
                continue;
            };
            if self.standing.get(name) != Some(&reason) {
                lines.push(format!(
                    "{name} feed failing for {}m: {reason}",
                    now.saturating_sub(status.last_success_secs) / 60
                ));
            }
            still.insert(name, reason);
        }
        lines.extend(
            self.standing
                .keys()
                .filter(|name| !still.contains_key(*name))
                .map(|name| format!("cleared: {name} feed back")),
        );
        self.standing = still;
        lines
    }
}

/// The reason a source has been failing for long enough to mention.
fn failing_long_enough(status: &SourceStatus, now: u64) -> Option<String> {
    let reason = status.error.clone()?;
    (now.saturating_sub(status.last_success_secs) >= SOURCE_NOTE_AFTER_SECS).then_some(reason)
}

/// Whether the last tick was reported as slow, so the report is made once
/// per stretch rather than once per tick.
#[derive(Debug, Default)]
pub(super) struct SlowTickWatch {
    slow: bool,
}

impl SlowTickWatch {
    /// What to log about a tick that took `took`, or `None` when nothing
    /// changed.
    ///
    /// A tick longer than the gap before the next one means that next one
    /// starts late and the scores on screen are older than the badge says.
    /// The gap is worked out here from `interval` and the run of `failures`
    /// behind it, rather than taken as the base interval: the loop backs off
    /// while a feed is down, so a 40s tick inside a 60s backoff is on time.
    /// Comparing it against the 30s base logged a slow tick and a recovery
    /// during exactly the outage a reader is trying to follow.
    pub(super) fn observe(
        &mut self,
        took: Duration,
        interval: u64,
        failures: u32,
    ) -> Option<String> {
        let cadence = backoff_secs(interval, failures);
        let slow = took > Duration::from_secs(cadence);
        if slow && !self.slow {
            self.slow = true;
            return Some(format!(
                "season tick took {:.1}s, longer than the {cadence}s until the next one: the scores on screen are older than the badge says",
                took.as_secs_f64(),
            ));
        }
        if !slow && self.slow {
            self.slow = false;
            return Some("season tick back inside its interval".to_string());
        }
        None
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn health(rosters_error: Option<&str>, rosters_ok_at: u64, now: u64) -> SourceHealth {
        let mut sources = SourceHealth::default();
        sources.matchups.succeeded(now);
        sources.scores.succeeded(now);
        sources.rosters.last_success_secs = rosters_ok_at;
        sources.rosters.error = rosters_error.map(str::to_string);
        sources
    }

    /// The bug: rosters could time out on every tick for an hour while the
    /// other two answered, and the poll health, which only counts ticks where
    /// everything failed, read green the whole time.
    #[test]
    fn one_source_failing_long_enough_is_reported_once_and_its_recovery_once() {
        let mut watch = SourceWatch::default();
        let ok_at = 10_000;
        // Two minutes in: too soon to mention, a blip is not an outage.
        assert!(watch
            .observe(
                Some(&health(Some("timeout"), ok_at, ok_at + 120)),
                ok_at + 120
            )
            .is_empty());
        let first = watch.observe(
            Some(&health(
                Some("timeout"),
                ok_at,
                ok_at + SOURCE_NOTE_AFTER_SECS,
            )),
            ok_at + SOURCE_NOTE_AFTER_SECS,
        );
        assert_eq!(
            first,
            vec!["rosters feed failing for 5m: timeout".to_string()]
        );
        for minute in 6..60 {
            let now = ok_at + minute * 60;
            assert!(
                watch
                    .observe(Some(&health(Some("timeout"), ok_at, now)), now)
                    .is_empty(),
                "the same failure at minute {minute} is not news"
            );
        }
        // A different reason is.
        let changed = watch.observe(
            Some(&health(Some("503"), ok_at, ok_at + 3_600)),
            ok_at + 3_600,
        );
        assert_eq!(
            changed,
            vec!["rosters feed failing for 60m: 503".to_string()]
        );
        // And coming back is, once.
        let back = watch.observe(
            Some(&health(None, ok_at + 3_630, ok_at + 3_630)),
            ok_at + 3_630,
        );
        assert_eq!(back, vec!["cleared: rosters feed back".to_string()]);
        assert!(watch
            .observe(
                Some(&health(None, ok_at + 3_660, ok_at + 3_660)),
                ok_at + 3_660
            )
            .is_empty());
    }

    #[test]
    fn no_season_loaded_clears_the_slate_without_a_line() {
        let mut watch = SourceWatch::default();
        let ok_at = 10_000;
        watch.observe(
            Some(&health(
                Some("timeout"),
                ok_at,
                ok_at + SOURCE_NOTE_AFTER_SECS,
            )),
            ok_at + SOURCE_NOTE_AFTER_SECS,
        );
        assert!(watch.observe(None, ok_at + 600).is_empty());
        assert!(
            watch
                .observe(Some(&health(None, ok_at + 700, ok_at + 700)), ok_at + 700)
                .is_empty(),
            "nothing remembered, so a healthy tick is not a recovery"
        );
    }

    /// A season tick that ran 40 seconds under a green badge left no trace.
    #[test]
    fn a_slow_tick_is_reported_once_and_its_recovery_once() {
        let mut watch = SlowTickWatch::default();
        assert_eq!(watch.observe(Duration::from_secs(4), 30, 0), None);
        let slow = watch
            .observe(Duration::from_secs(40), 30, 0)
            .expect("the first slow tick is reported");
        assert!(
            slow.contains("40.0s") && slow.contains("30s until"),
            "{slow}"
        );
        assert_eq!(watch.observe(Duration::from_secs(41), 30, 0), None);
        assert_eq!(
            watch.observe(Duration::from_secs(3), 30, 0).as_deref(),
            Some("season tick back inside its interval")
        );
        assert_eq!(watch.observe(Duration::from_secs(3), 30, 0), None);
    }

    /// The bug: the tick was measured against the base interval rather than
    /// the gap the loop actually sleeps for, so during a backoff every tick
    /// that ran longer than 30s was logged as slow and then "recovered" a
    /// tick later, in the middle of the outage a reader is trying to follow.
    #[test]
    fn a_tick_inside_the_backoff_it_is_running_under_is_not_slow() {
        let mut watch = SlowTickWatch::default();
        // Three failures in a row: the loop waits 60s, not 30s, before the
        // next tick, so a 40s tick is comfortably on time.
        assert_eq!(watch.observe(Duration::from_secs(40), 30, 3), None);
        // Past the real cadence it is slow again, and says the real number.
        let slow = watch
            .observe(Duration::from_secs(70), 30, 3)
            .expect("70s is longer than the 60s the loop is waiting");
        assert!(slow.contains("60s until"), "{slow}");
    }
}
