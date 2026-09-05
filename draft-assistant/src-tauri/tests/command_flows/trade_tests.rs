//! Two things the draft screen has to get right after the league has already
//! loaded: a pick that changes hands while the draft is running, and an
//! export that has to survive a disk which will not take it.

use super::session;
use crate::routes::{LEAGUE_ID, LEAGUE_TRADE, TRADE_AGREED};
use serde_json::{json, Value};
use std::sync::atomic::Ordering;

/// The picks the frontend draws as mine, in order.
fn my_picks(view: &Value) -> Vec<u64> {
    view["draft"]["my_next_picks"]
        .as_array()
        .expect("the view says which picks are mine")
        .iter()
        .map(|p| p.as_u64().expect("a pick number"))
        .collect()
}

/// The trade list was read once, at load. Managers agree trades *during* a
/// draft, and from the moment they did the screen kept drawing the old owner
/// on the clock and kept counting a pick that was no longer mine among my
/// next picks — at the one hour of the season where that costs a pick.
#[test]
fn a_trade_agreed_mid_draft_moves_the_pick_on_the_next_refresh() {
    TRADE_AGREED.store(false, Ordering::SeqCst);
    let s = session("mid-draft-trade");
    s.ok("set_my_username", json!({"username": "ada"}));
    let view = s.ok(
        "add_league",
        json!({"leagueId": LEAGUE_TRADE, "force": true}),
    );
    // Two teams over three rounds, and I am slot 1: picks 1, 4 and 5.
    assert_eq!(view["draft"]["my_slot"], 1);
    assert_eq!(my_picks(&view), vec![1, 4, 5]);
    assert_eq!(
        view["draft"]["pick_slot_overrides"],
        json!({}),
        "nothing has changed hands yet"
    );

    // Slot 1 sells its third round to slot 2 while the draft is running.
    TRADE_AGREED.store(true, Ordering::SeqCst);
    let refreshed = s.ok("refresh_picks", json!({}));
    assert_eq!(
        refreshed["draft"]["pick_slot_overrides"]["5"], 2,
        "the pick belongs to slot 2 now: {}",
        refreshed["draft"]["pick_slot_overrides"]
    );
    assert_eq!(
        my_picks(&refreshed),
        vec![1, 4],
        "a pick I sold is not one I am waiting for"
    );

    // And it stays sold: get_state re-renders from what the tick adopted.
    assert_eq!(my_picks(&s.ok("get_state", json!({}))), vec![1, 4]);
    TRADE_AGREED.store(false, Ordering::SeqCst);
    s.finish();
}

/// The export is the whole league — every member's name, every Sleeper user
/// id, every roster — and it was written with a plain `fs::write` at the
/// default 0644: readable by every account on the machine, and truncated
/// before the new bytes existed, so a disk that would not take the write left
/// the user with an empty file instead of the export they already had.
#[cfg(unix)]
#[test]
fn an_export_is_owner_only_and_a_failed_one_keeps_the_last_good_file() {
    use std::os::unix::fs::PermissionsExt;

    let s = session("export-atomic");
    s.ok("add_league", json!({"leagueId": LEAGUE_ID, "force": true}));
    let path = s.ok("export_state", json!({}));
    let path = std::path::PathBuf::from(path.as_str().expect("a path came back"));
    let mode = std::fs::metadata(&path)
        .expect("the export exists")
        .permissions()
        .mode()
        & 0o777;
    assert_eq!(mode, 0o600, "the export is readable by anyone: {mode:o}");
    let first = std::fs::read_to_string(&path).expect("the export reads back");
    assert!(first.contains("Command League"));

    // A directory nothing can be created in is the full or read-only disk
    // this has to survive.
    let locked = std::fs::Permissions::from_mode(0o500);
    std::fs::set_permissions(&s.data_dir, locked).expect("the directory locks");
    let blocked = std::fs::File::create(s.data_dir.join("probe")).is_err();
    let error = s.invoke("export_state", json!({})).err();
    std::fs::set_permissions(&s.data_dir, std::fs::Permissions::from_mode(0o700))
        .expect("the directory unlocks");
    assert!(
        blocked,
        "the test cannot lock its own directory, so it proves nothing"
    );
    assert!(
        error.is_some(),
        "an export that could not be written said so"
    );
    assert_eq!(
        std::fs::read_to_string(&path).expect("the previous export is still there"),
        first,
        "a failed export truncated the one already on disk"
    );
    s.finish();
}
