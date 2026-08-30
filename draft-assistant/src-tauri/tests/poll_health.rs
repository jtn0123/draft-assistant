//! How a poll tick records success and failure on the league it is watching.

mod common;

use draft_assistant_lib::poll::record_poll_outcome;

#[test]
fn a_clean_tick_clears_the_failure_run_and_a_bad_one_extends_it() {
    let (mut loaded, _, _) = common::fixture();
    loaded.poll_consecutive_failures = 4;
    loaded.poll_last_error = Some("earlier".into());

    record_poll_outcome(&mut loaded, &[]);
    assert_eq!(loaded.poll_consecutive_failures, 0);
    assert!(loaded.poll_last_error.is_none());
    assert!(loaded.poll_last_success_at.is_some());

    record_poll_outcome(&mut loaded, &["picks: timeout".into()]);
    assert_eq!(loaded.poll_consecutive_failures, 1);
    record_poll_outcome(&mut loaded, &["picks: timeout".into(), "draft: 502".into()]);
    assert_eq!(loaded.poll_consecutive_failures, 2);
    // Every reason is kept, not just the last one.
    let error = loaded.poll_last_error.clone().unwrap();
    assert!(
        error.contains("timeout") && error.contains("502"),
        "lost a reason: {error}"
    );
}

#[test]
fn a_failure_never_backdates_the_last_success() {
    let (mut loaded, _, _) = common::fixture();
    record_poll_outcome(&mut loaded, &[]);
    let succeeded_at = loaded.poll_last_success_at;
    record_poll_outcome(&mut loaded, &["down".into()]);
    assert_eq!(
        loaded.poll_last_success_at, succeeded_at,
        "a failing tick must not move the last-success time"
    );
}
