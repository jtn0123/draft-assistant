//! Tests for stopping an answer: the claim a question holds while it is in
//! flight, and the Cancel button that reaches it by screen.
//!
//! Split from `commands_chat_tests.rs`, which was at the line cap.

use super::*;

/// Two questions asked at the same moment both read the spend from before
/// either of them, so both passed a cap with room for only one.
#[test]
fn two_questions_about_one_league_cannot_be_in_flight_together() {
    let claims = crate::chat_client::InFlightClaims::default();
    let key = spend_key("draft", Some("in-flight-league"));
    let held = claims.reserve(&key).expect("the first is accepted");
    let error = claims.reserve(&key).expect_err("the second is refused");
    assert_eq!(error, crate::chat_client::BUSY_MESSAGE);
    // The other screen of the same league keeps its own claim.
    claims
        .reserve(&spend_key("season", Some("in-flight-league")))
        .expect("the season screen is free");
    drop(held);
    claims
        .reserve(&key)
        .expect("the claim is released when the turn ends");
}

/// The Cancel button names a screen, and the command finds the claim by the
/// same key the answer was claimed under. Nothing in flight is a plain
/// `false`: the answer may have landed a moment before the click.
#[tokio::test]
async fn cancelling_a_screen_reaches_the_claim_its_answer_holds() {
    let (state, _dir) = AppState::scratch("chat-cancel");
    state.config.lock().await.active_league_id = Some("cancel-league".to_string());
    assert!(!cancel_claude_inner(&state, "draft")
        .await
        .expect("a screen with nothing in flight"));
    let held = claim(&state, "draft").await.expect("the claim is free");
    assert_eq!(held.key(), "draft.cancel-league");
    let signal = held.signal();
    assert!(!signal.is_cancelled());
    assert!(cancel_claude_inner(&state, "draft")
        .await
        .expect("the screen is a screen"));
    assert!(signal.is_cancelled(), "the answer's signal was not pulled");
    // The other screen's answer is not the one that was stopped.
    let season = claim(&state, "season").await.expect("its own claim");
    assert!(!season.signal().is_cancelled());
    // And a name that is not a screen is refused rather than looked up.
    assert!(cancel_claude_inner(&state, "settings").await.is_err());
}

/// A claim made before the question is filed is the one the answer runs
/// under: a second claim for the same board is refused while it is held.
#[tokio::test]
async fn a_claim_made_up_front_is_the_one_the_answer_holds() {
    let (state, _dir) = AppState::scratch("chat-claim-up-front");
    state.config.lock().await.active_league_id = Some("claim-league".to_string());
    let held = claim(&state, "draft").await.expect("claimed");
    assert_eq!(
        claim(&state, "draft").await.unwrap_err(),
        crate::chat_client::BUSY_MESSAGE
    );
    // Handing the claim to the answer path does not double-claim: the call
    // fails on the empty thread, and the claim is released with it.
    let error = answer_holding(&state, "draft", "", "", Vec::new(), held)
        .await
        .expect_err("an empty thread is nothing to ask");
    assert!(error.contains("nothing to ask"), "{error}");
    claim(&state, "draft")
        .await
        .expect("released with the answer");
}
