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

#[test]
fn the_waits_of_one_call_never_add_up_to_more_than_the_budget() {
    // The failure this prevents: five attempts each honouring a thirty-second
    // `Retry-After` is two minutes of sleeping inside one poll tick, and the
    // tick has no timeout of its own, so the board stopped dead.
    let policy = RetryPolicy {
        jitter: false,
        ..RetryPolicy::default()
    };
    let mut spent = Duration::ZERO;
    let mut taken = Vec::new();
    for attempt in 1..policy.attempts {
        let Some(wait) = policy.wait_within(attempt, None, spent) else {
            break;
        };
        spent += wait;
        taken.push(wait.as_secs());
    }
    assert_eq!(
        taken,
        vec![1, 2, 4],
        "8s would run past the ten-second budget"
    );
    assert!(
        spent <= policy.budget,
        "{spent:?} is past {:?}",
        policy.budget
    );

    // Yahoo asking for its full cap is refused outright rather than slept.
    assert_eq!(
        policy.wait_within(1, Some(Duration::from_secs(600)), Duration::ZERO),
        None,
        "a thirty-second wait does not fit in a ten-second budget"
    );
    // A wait that fits is still taken.
    assert_eq!(
        policy.wait_within(1, Some(Duration::from_secs(2)), Duration::ZERO),
        Some(Duration::from_secs(2))
    );
    // …but not once the budget has already gone.
    assert_eq!(
        policy.wait_within(1, Some(Duration::from_secs(2)), policy.budget),
        None
    );
}

#[test]
fn a_throttle_is_reported_for_a_while_and_then_stops_being_reported() {
    let log = ThrottleLog::default();
    assert_eq!(log.warning(1_000), None, "nothing has been throttled yet");
    log.note(1_000);
    assert_eq!(log.warning(1_000), Some(THROTTLED));
    assert_eq!(
        log.warning(1_000 + THROTTLE_NOTICE_SECS - 1),
        Some(THROTTLED)
    );
    assert_eq!(
        log.warning(1_000 + THROTTLE_NOTICE_SECS),
        None,
        "the line has to go away on its own once Yahoo lets go"
    );
    // The sentence is for the board, so it says what it means rather than
    // repeating Yahoo's own status, which means nothing to anybody.
    assert!(!THROTTLED.contains("999"), "{THROTTLED}");
    assert!(THROTTLED.contains("Yahoo"), "{THROTTLED}");
}
