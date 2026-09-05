use super::*;

#[test]
fn the_backoff_doubles_from_a_second_rather_than_giving_up_in_under_one() {
    // The failure this prevents: 250ms then 500ms then failure, which is
    // shorter than every throttle Yahoo has ever applied.
    let policy = RetryPolicy {
        jitter: false,
        ..RetryPolicy::default()
    };
    let waits: Vec<u64> = (1..=5).map(|n| policy.wait(n, None).as_secs()).collect();
    assert_eq!(waits, vec![1, 2, 4, 8, 16]);
    assert_eq!(policy.attempts, 5);
}

#[test]
fn a_wait_is_never_longer_than_the_cap_even_when_yahoo_asks_for_one() {
    let policy = RetryPolicy {
        jitter: false,
        ..RetryPolicy::default()
    };
    assert_eq!(
        policy.wait(1, Some(Duration::from_secs(600))),
        policy.cap,
        "a ten-minute Retry-After would hold the draft board hostage"
    );
    assert_eq!(policy.wait(9, None), policy.cap);
}

#[test]
fn yahoos_own_retry_after_wins_over_the_curve() {
    let policy = RetryPolicy {
        jitter: false,
        ..RetryPolicy::default()
    };
    assert_eq!(
        policy.wait(3, Some(Duration::from_secs(2))),
        Duration::from_secs(2),
        "Yahoo knows how long its own throttle has left"
    );
}

#[test]
fn jitter_stays_inside_the_quarter_it_promises() {
    let policy = RetryPolicy::default();
    for _ in 0..20 {
        let wait = policy.wait(1, None);
        assert!(
            wait >= Duration::from_secs(1) && wait <= Duration::from_millis(1_250),
            "{wait:?}"
        );
    }
}

#[test]
fn retry_after_reads_both_spellings() {
    assert_eq!(retry_after("1", 0), Some(Duration::from_secs(1)));
    assert_eq!(retry_after(" 120 ", 0), Some(Duration::from_secs(120)));
    // 2015-10-21T07:28:00Z is 1445412480.
    let at = 1_445_412_480;
    assert_eq!(
        retry_after("Wed, 21 Oct 2015 07:28:00 GMT", at - 30),
        Some(Duration::from_secs(30))
    );
    // A date that has already passed asks for no wait at all, not an error.
    assert_eq!(
        retry_after("Wed, 21 Oct 2015 07:28:00 GMT", at + 5),
        Some(Duration::ZERO)
    );
    assert_eq!(retry_after("soon", 0), None);
    assert_eq!(retry_after("", 0), None);
}

#[test]
fn the_epoch_arithmetic_matches_known_dates() {
    assert_eq!(
        retry_after("Thu, 01 Jan 1970 00:00:00 GMT", 0),
        Some(Duration::ZERO)
    );
    assert_eq!(
        retry_after("Sun, 06 Nov 1994 08:49:37 GMT", 0),
        Some(Duration::from_secs(784_111_777))
    );
    // A leap day, which is where hand-rolled date maths usually goes wrong.
    assert_eq!(
        retry_after("Sat, 29 Feb 2020 00:00:00 GMT", 0),
        Some(Duration::from_secs(1_582_934_400))
    );
}

#[test]
fn the_fast_policy_retries_a_known_number_of_times_and_waits_in_milliseconds() {
    let fast = RetryPolicy::fast();
    // The count is what the retry tests count against, so it is asserted here
    // rather than assumed: a change to `fast()` that silently added or
    // dropped an attempt would move every "tried N times" assertion with it.
    assert_eq!(fast.attempts, 3);
    assert!(
        fast.attempts < RetryPolicy::default().attempts,
        "the shipped policy is the patient one"
    );
    assert!(fast.wait(3, None) <= Duration::from_millis(50));
    assert!(!fast.jitter, "a test asserting on a sleep wants no jitter");
}
