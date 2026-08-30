//! Pricing an offer through `AppCore` — the path the desktop command calls.
//! Split out of `app_core.rs` for the 500-line cap.

mod support;

use support::{loaded_rig, pick};

#[tokio::test]
async fn an_offer_naming_a_player_nobody_holds_is_refused_with_the_reason() {
    let rig = loaded_rig("evaluate-trade").await;
    let err = rig
        .core
        .evaluate_trade(2, vec!["nobody".into()], Vec::new(), Vec::new(), Vec::new())
        .await
        .unwrap_err();
    assert!(err.contains("not on my roster"), "{err}");
    let err = rig
        .core
        .evaluate_trade(2, Vec::new(), Vec::new(), Vec::new(), Vec::new())
        .await
        .unwrap_err();
    assert!(err.contains("at least one player or pick"), "{err}");
}

/// The whole pricing path through `AppCore` — the one the desktop command
/// calls — with a draft pick in the offer. Unit tests price `evaluate` on a
/// hand-built league; this proves the round survives the trip from the
/// command's arguments to a verdict on a real loaded league.
#[tokio::test]
async fn an_offer_with_a_draft_pick_comes_back_priced() {
    let rig = loaded_rig("evaluate-trade-pick").await;
    rig.fixture
        .set_picks(&rig.stub, &[pick(1, 1, "qb-1"), pick(2, 2, "rb-1")]);
    rig.core.refresh_picks().await.unwrap();
    let view = rig.core.get_state().await.unwrap();
    let prices = &view.pick_prices;
    assert!(!prices.is_empty(), "a drafted round has a price");
    let round = prices[0].round;
    let mine = view.my_roster.as_ref().unwrap().players[0]
        .player_id
        .clone();
    let verdict = rig
        .core
        .evaluate_trade(2, vec![mine], Vec::new(), Vec::new(), vec![round])
        .await
        .unwrap();
    assert_eq!(verdict.get_picks.len(), 1);
    assert_eq!(verdict.get_picks[0].round, round);
    assert!(
        (verdict.get_picks[0].points - prices[0].points).abs() < 1e-9,
        "the verdict prices the round the same way the view does: {:?} vs {:?}",
        verdict.get_picks[0],
        prices[0]
    );
    assert!(verdict.give_picks.is_empty());
    // A round this draft never had is refused by name, not silently priced 0.
    let err = rig
        .core
        .evaluate_trade(2, Vec::new(), Vec::new(), Vec::new(), vec![99])
        .await
        .unwrap_err();
    assert!(err.contains("no round 99"), "{err}");
}
