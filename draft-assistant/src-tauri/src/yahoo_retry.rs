//! How long to wait before repeating a Yahoo request.
//!
//! Yahoo throttles a busy client with its own status 999 and, when it feels
//! like it, a `Retry-After` header. The old policy here waited 250ms and then
//! 500ms and gave up: a throttle that Yahoo said would last a second outlived
//! all three attempts, and the whole load failed while the draft was running.
//!
//! So: honour `Retry-After` when Yahoo sends one (both spellings — a count of
//! seconds, and an HTTP date), and otherwise double the wait from a second up
//! to sixteen. Jitter is added because every page of the player pool fails at
//! the same moment, and unjittered backoff would send all of them back at the
//! same moment too.
//!
//! Pure functions and a plain struct: no clock of its own beyond the `now` a
//! caller passes in, so the date arithmetic is testable without waiting.

use std::time::Duration;

/// The waits, and how many of them. Overridable so a test does not sit
/// through the real thing.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct RetryPolicy {
    /// Total attempts per request, including the first.
    pub attempts: u32,
    /// The wait after the first failure; doubles from there.
    pub base: Duration,
    /// The longest this will ever wait, `Retry-After` included. A Yahoo that
    /// asks for ten minutes is not worth holding the draft board for.
    pub cap: Duration,
    /// Whether to spread the wait out. Off in tests so a sleep is exact.
    pub jitter: bool,
}

impl Default for RetryPolicy {
    fn default() -> Self {
        Self {
            attempts: 5,
            base: Duration::from_secs(1),
            cap: Duration::from_secs(30),
            jitter: true,
        }
    }
}

impl RetryPolicy {
    /// A policy that retries as often but waits in milliseconds, for tests.
    pub fn fast() -> Self {
        Self {
            attempts: 3,
            base: Duration::from_millis(5),
            cap: Duration::from_millis(50),
            jitter: false,
        }
    }

    /// The wait before attempt number `attempt` + 1, where `attempt` counts
    /// the failures so far (1 after the first).
    ///
    /// `asked_for` is Yahoo's own `Retry-After`, which wins over the curve
    /// when it is there: Yahoo knows how long its own throttle has left.
    pub fn wait(&self, attempt: u32, asked_for: Option<Duration>) -> Duration {
        let base = match asked_for {
            Some(wait) => wait,
            None => {
                let doublings = attempt.saturating_sub(1).min(20);
                self.base.saturating_mul(1u32 << doublings)
            }
        };
        let capped = base.min(self.cap);
        if !self.jitter {
            return capped;
        }
        // Full jitter over the top quarter: still roughly the curve, but two
        // requests that failed together no longer come back together.
        let spread = capped.as_millis() as u64 / 4;
        capped + Duration::from_millis(pseudo_random(spread))
    }
}

/// A number in `0..=range`, from the clock's low bits. Not random enough for
/// anything but spreading retries out, which is all it is for.
fn pseudo_random(range: u64) -> u64 {
    if range == 0 {
        return 0;
    }
    let nanos = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map(|since| since.subsec_nanos() as u64)
        .unwrap_or(0);
    nanos % (range + 1)
}

/// `Retry-After` as a wait: either delta-seconds ("120") or an HTTP date
/// ("Wed, 21 Oct 2015 07:28:00 GMT"), which is read against `now`.
///
/// A date already in the past is no wait at all rather than an error: the
/// caller is meant to try again, just not to sleep first.
pub fn retry_after(header: &str, now: u64) -> Option<Duration> {
    let header = header.trim();
    if let Ok(seconds) = header.parse::<u64>() {
        return Some(Duration::from_secs(seconds));
    }
    let at = http_date(header)?;
    Some(Duration::from_secs(at.saturating_sub(now)))
}

/// An IMF-fixdate (`Sun, 06 Nov 1994 08:49:37 GMT`) as epoch seconds.
///
/// Only that one spelling: it is the only one Yahoo has ever sent, and the two
/// obsolete formats in the RFC would double this for nothing. An unparseable
/// header is `None`, which puts the caller back on the plain backoff curve.
fn http_date(header: &str) -> Option<u64> {
    let rest = header
        .split_once(", ")
        .map(|(_, rest)| rest)
        .unwrap_or(header);
    let mut parts = rest.split_whitespace();
    let day: i64 = parts.next()?.parse().ok()?;
    let month = month_number(parts.next()?)?;
    let year: i64 = parts.next()?.parse().ok()?;
    let mut clock = parts.next()?.split(':');
    let hour: u64 = clock.next()?.parse().ok()?;
    let minute: u64 = clock.next()?.parse().ok()?;
    let second: u64 = clock.next()?.parse().ok()?;
    let days = days_from_civil(year, month, day);
    let seconds = days * 86_400 + (hour * 3_600 + minute * 60 + second) as i64;
    u64::try_from(seconds).ok()
}

fn month_number(name: &str) -> Option<i64> {
    const MONTHS: [&str; 12] = [
        "Jan", "Feb", "Mar", "Apr", "May", "Jun", "Jul", "Aug", "Sep", "Oct", "Nov", "Dec",
    ];
    MONTHS
        .iter()
        .position(|month| month.eq_ignore_ascii_case(name))
        .map(|index| index as i64 + 1)
}

/// Days since 1970-01-01 for a proleptic Gregorian date (Howard Hinnant's
/// `days_from_civil`). Spelled out rather than pulled in as a dependency: it
/// is fifteen lines and it is the only date arithmetic this crate does.
fn days_from_civil(year: i64, month: i64, day: i64) -> i64 {
    let year = if month <= 2 { year - 1 } else { year };
    let era = if year >= 0 { year } else { year - 399 } / 400;
    let year_of_era = year - era * 400;
    let day_of_year = (153 * (if month > 2 { month - 3 } else { month + 9 }) + 2) / 5 + day - 1;
    let day_of_era = year_of_era * 365 + year_of_era / 4 - year_of_era / 100 + day_of_year;
    era * 146_097 + day_of_era - 719_468
}

#[cfg(test)]
#[path = "yahoo_retry_tests.rs"]
mod tests;
