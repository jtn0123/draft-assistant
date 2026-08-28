//! Manual picks: the offline fallback for when Sleeper's feed lags. What the
//! commands do to them, and — the part that broke on draft day — what the
//! poll running three seconds later is allowed to do to them.

mod support;

use support::{loaded_rig, make_rig, pick, DRAFT_ID, LEAGUE_ID, MY_USERNAME};

#[tokio::test]
async fn a_manual_pick_is_applied_persisted_and_undone_and_a_failed_save_rolls_back() {
    let rig = loaded_rig("manual").await;
    let view = rig.core.record_manual_pick("rb-1".into()).await.unwrap();
    assert!(view.draft.manual_picks_active);
    assert_eq!(view.draft.current_pick, 2);
    assert!(view.available.iter().all(|p| p.player.player_id != "rb-1"));
    assert_eq!(view.my_roster.unwrap().players[0].player_id, "rb-1");
    assert!(
        rig.core
            .engine
            .data_dir
            .join("manual_picks_fixture-draft.json")
            .is_file(),
        "the pick is on disk before it is on screen"
    );
    let err = rig
        .core
        .record_manual_pick("no-such-player".into())
        .await
        .unwrap_err();
    assert!(!err.is_empty());

    let view = rig.core.undo_manual_pick().await.unwrap();
    assert!(!view.draft.manual_picks_active);
    assert_eq!(view.available.len(), 6);
    assert!(
        rig.core.undo_manual_pick().await.is_err(),
        "nothing left to undo"
    );

    // The data directory vanishes: the pick must not appear on the board.
    std::fs::remove_dir_all(&rig.core.engine.data_dir).unwrap();
    let err = rig
        .core
        .record_manual_pick("wr-1".into())
        .await
        .unwrap_err();
    assert!(err.contains("write"), "{err}");
    let view = rig.core.get_state().await.unwrap();
    assert!(!view.draft.manual_picks_active);
    assert_eq!(view.available.len(), 6);
}

#[tokio::test]
async fn live_picks_supersede_manual_ones() {
    let rig = loaded_rig("supersede").await;
    rig.core.record_manual_pick("rb-1".into()).await.unwrap();
    // The API now reports pick 1 as someone else; the manual pick is dropped.
    rig.fixture.set_picks(&rig.stub, &[pick(1, 1, "qb-1")]);
    let view = rig.core.refresh_picks().await.unwrap();
    assert!(!view.draft.manual_picks_active);
    assert_eq!(view.draft.total_picks_made, 1);
    assert!(rig.core.undo_manual_pick().await.is_err());
}

#[tokio::test]
async fn a_manual_pick_in_a_keeper_league_survives_the_poll_that_follows_it() {
    // The live bug: the real league's feed opens with keepers scattered up to
    // pick 195, and the poll three seconds later retired the manual pick at
    // pick 1 — the board kept showing it, and Undo said there was nothing to
    // undo. The fallback the user leans on when Sleeper lags was gone.
    let rig = make_rig("manual-vs-keepers");
    rig.fixture
        .set_picks(&rig.stub, &[pick(5, 1, "qb-1"), pick(8, 2, "te-1")]);
    rig.core.add_league(LEAGUE_ID, false).await.unwrap();
    rig.core.set_my_username(MY_USERNAME).await.unwrap();

    let view = rig.core.record_manual_pick("rb-1".into()).await.unwrap();
    assert_eq!(view.draft.current_pick, 2, "pick 1 was ours to fill");
    assert!(view.draft.manual_picks_active);

    // A poll with an unchanged feed must leave it alone, on disk and in view.
    let mut fingerprint = None;
    let (_, emitted) = rig.core.poll_once(&mut fingerprint).await.unwrap();
    assert!(
        emitted.is_some(),
        "first poll has nothing to compare against"
    );
    let (_, emitted) = rig.core.poll_once(&mut fingerprint).await.unwrap();
    assert!(emitted.is_none(), "an unchanged feed emits nothing");
    let after = rig.core.get_state().await.unwrap();
    assert!(after.draft.manual_picks_active, "the manual pick survived");
    assert_eq!(after.draft.current_pick, 2);
    assert_eq!(rig.core.engine.load_manual_picks(DRAFT_ID).len(), 1);

    // Undo still has something to undo, and the board goes back.
    let view = rig.core.undo_manual_pick().await.unwrap();
    assert_eq!(view.draft.current_pick, 1);
    assert!(!view.draft.manual_picks_active);
}

#[tokio::test]
async fn a_poll_that_retires_a_manual_pick_emits_the_corrected_view() {
    // The second half of the same bug: when Sleeper finally reports the pick
    // the user recorded by hand, the poll drops the manual copy — and must
    // say so, or the UI renders a pick the backend no longer holds.
    let rig = loaded_rig("manual-retired").await;
    rig.core.record_manual_pick("rb-1".into()).await.unwrap();
    let mut fingerprint = None;
    rig.core.poll_once(&mut fingerprint).await.unwrap();

    // Sleeper now has that same pick number, from the API.
    rig.fixture.set_picks(&rig.stub, &[pick(1, 1, "wr-1")]);
    let (_, emitted) = rig.core.poll_once(&mut fingerprint).await.unwrap();
    let emitted = emitted.expect("the feed changed, so a view is emitted");
    assert!(
        !emitted.draft.manual_picks_active,
        "the manual copy is gone"
    );
    assert!(rig.core.engine.load_manual_picks(DRAFT_ID).is_empty());

    // And when only the manual side changes, the poll still emits: the feed
    // is unchanged across these two polls, but the board is not what it was.
    let rig = loaded_rig("manual-retired-quietly").await;
    rig.fixture.set_picks(&rig.stub, &[pick(5, 1, "qb-1")]);
    let mut fingerprint = None;
    rig.core.poll_once(&mut fingerprint).await.unwrap();
    let (_, quiet) = rig.core.poll_once(&mut fingerprint).await.unwrap();
    assert!(quiet.is_none(), "the feed settled");

    // A manual pick the feed already covers — the player is in the book.
    {
        let mut loaded = rig.core.loaded.lock().await;
        loaded
            .as_mut()
            .unwrap()
            .manual_picks
            .push(pick(2, 2, "qb-1"));
    }
    let (_, emitted) = rig.core.poll_once(&mut fingerprint).await.unwrap();
    assert!(
        emitted.is_some(),
        "the poll retired the manual pick, so the UI must be told"
    );
    assert!(!emitted.unwrap().draft.manual_picks_active);
}
