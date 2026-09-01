//! The league rules that change who picks where: keepers already in the book,
//! draft picks that changed hands, and third-round reversal.
//!
//! These drive the real `build_view`, because the bugs they are about were all
//! bugs of the assembled view — the clock several rounds ahead of itself, the
//! wrong manager named on the clock, a roster credited to the slot a pick
//! started with rather than the one that owns it now.

mod common;

use draft_assistant_lib::engine::LoadedLeague;
use draft_assistant_lib::sleeper::Pick;
use draft_assistant_lib::traded_picks::TradedPick;
use draft_assistant_lib::view::{build_view, DraftView};
use std::collections::HashMap;

/// The fixture is four teams over six rounds; u1 (me) is slot 1.
fn league() -> (LoadedLeague, draft_assistant_lib::engine::AppConfig) {
    let (loaded, _, config) = common::fixture();
    (loaded, config)
}

fn pick(pick_no: u32, player_id: &str, keeper: bool) -> Pick {
    Pick {
        round: (pick_no - 1) / 4 + 1,
        pick_no,
        draft_slot: (pick_no - 1) % 4 + 1,
        player_id: player_id.into(),
        picked_by: None,
        metadata: None,
        is_keeper: keeper.then_some(true),
    }
}

fn view(loaded: &LoadedLeague, config: &draft_assistant_lib::engine::AppConfig) -> DraftView {
    build_view(loaded, config)
}

#[test]
fn keepers_sitting_ahead_of_the_clock_do_not_advance_the_draft() {
    let (mut loaded, config) = league();
    // Two keepers, entered before anybody is on the clock: pick 6 and pick 19.
    loaded.api_picks = vec![pick(6, "w3", false), pick(19, "w7", false)];

    let v = view(&loaded, &config);
    // Counting picks would say pick 3. The board's first gap is pick 1.
    assert_eq!(v.draft.current_pick, 1);
    assert_eq!(v.draft.current_round, 1);
    assert_eq!(v.draft.on_clock_slot, 1);
    assert!(v.draft.is_my_pick, "slot 1 is mine and pick 1 is open");
    assert_eq!(v.draft.keeper_picks, vec![6, 19]);
    // The keepers are not news — they happened before the draft did.
    assert!(v.recent_picks.is_empty(), "{:?}", v.recent_picks);
}

#[test]
fn a_keeper_is_tagged_on_the_roster_and_taken_off_the_board() {
    let (mut loaded, config) = league();
    // Slot 2's keeper at pick 6 (round 2, slot 3 in a snake — but `picked_by`
    // is absent, so ownership decides), plus a real first round.
    loaded.api_picks = vec![pick(6, "w3", false)];

    let v = view(&loaded, &config);
    assert!(
        !v.available.iter().any(|p| p.player.player_id == "w3"),
        "a kept player is not draftable"
    );
    let holder = v
        .rosters
        .iter()
        .find(|r| r.players.iter().any(|p| p.player_id == "w3"))
        .expect("the keeper lands on somebody's roster");
    let entry = &holder.players[0];
    assert!(entry.is_keeper, "kept, not drafted tonight");
    assert_eq!(entry.pick_no, 6);
}

#[test]
fn keepers_in_the_way_are_not_counted_as_picks_i_am_waiting_for() {
    let (mut loaded, config) = league();
    // Picks 1..=3 drafted; 4 is a keeper. Slot 1 (mine) picks again at 8.
    loaded.api_picks = vec![
        pick(1, "q1", false),
        pick(2, "r1", false),
        pick(3, "w1", false),
        pick(4, "w2", true),
    ];

    let v = view(&loaded, &config);
    assert_eq!(v.draft.current_pick, 5);
    assert_eq!(v.draft.my_next_picks.first().copied(), Some(8));
    // 5, 6, 7 — three picks. Pick 4 is behind us and was never anybody's turn.
    assert_eq!(v.draft.picks_until_mine, Some(3));

    // And with a keeper *between* here and my pick, it is not a wait either.
    loaded.api_picks.push(pick(6, "w4", true));
    let v = view(&loaded, &config);
    assert_eq!(v.draft.picks_until_mine, Some(2));
}

/// Roster ids are deliberately not equal to slots, so a test that confused
/// the two would fail: slot 1 -> roster 40, slot 2 -> roster 30, and so on.
fn with_roster_map(loaded: &mut LoadedLeague) {
    loaded.draft.slot_to_roster_id = Some(HashMap::from([
        ("1".to_string(), 40),
        ("2".to_string(), 30),
        ("3".to_string(), 20),
        ("4".to_string(), 10),
    ]));
}

fn traded(round: u32, roster_id: u32, owner_id: u32) -> TradedPick {
    TradedPick {
        season: "2025".into(),
        round,
        roster_id,
        owner_id,
        previous_owner_id: Some(roster_id),
    }
}

#[test]
fn a_pick_i_traded_away_is_not_mine_and_one_i_acquired_is() {
    let (mut loaded, config) = league();
    with_roster_map(&mut loaded);
    // I (slot 1, roster 40) sent my round 2 to slot 3 (roster 20), and took
    // slot 4's round 3 (roster 10) in return.
    loaded.traded_picks = vec![traded(2, 40, 20), traded(3, 10, 40)];

    let v = view(&loaded, &config);
    // Snake, 4 teams: round 2 is picks 5–8 running 4,3,2,1 — mine is 8.
    // Round 3 is picks 9–12 running 1,2,3,4 — slot 4's is 12.
    assert!(!v.draft.my_next_picks.contains(&8), "traded away");
    assert!(v.draft.my_next_picks.contains(&12), "acquired");
    assert_eq!(v.draft.my_next_picks, vec![1, 9, 12, 16, 17, 24]);
    assert_eq!(v.draft.pick_slot_overrides.get(&8), Some(&3));
    assert_eq!(v.draft.pick_slot_overrides.get(&12), Some(&1));
    assert_eq!(v.draft.pick_slot_overrides.len(), 2);
}

#[test]
fn the_manager_on_the_clock_is_the_one_who_owns_the_pick() {
    let (mut loaded, config) = league();
    with_roster_map(&mut loaded);
    loaded.traded_picks = vec![traded(1, 40, 20)];
    // Pick 1 belongs to slot 1 in the snake, but slot 3 owns it now.
    let v = view(&loaded, &config);
    assert_eq!(v.draft.current_pick, 1);
    assert_eq!(v.draft.on_clock_slot, 3);
    assert!(!v.draft.is_my_pick, "it is not my turn any more");
    assert_eq!(v.draft.on_clock_name.as_deref(), Some("User Three"));
}

#[test]
fn a_traded_pick_lands_on_the_roster_that_owns_it() {
    let (mut loaded, config) = league();
    with_roster_map(&mut loaded);
    loaded.traded_picks = vec![traded(1, 40, 20)];
    // No `picked_by` on the pick, so ownership is the only guide.
    loaded.api_picks = vec![pick(1, "q1", false)];

    let v = view(&loaded, &config);
    let slot_three = &v.rosters[2];
    assert_eq!(slot_three.slot, 3);
    assert_eq!(slot_three.players.len(), 1, "{:?}", slot_three.players);
    assert_eq!(slot_three.players[0].player_id, "q1");
    assert!(v.rosters[0].players.is_empty(), "not the original slot");
    assert_eq!(v.recent_picks[0].slot, 3);
    assert_eq!(v.recent_picks[0].slot_name.as_deref(), Some("User Three"));
}

#[test]
fn a_pick_that_names_its_drafter_beats_the_ownership_guess() {
    let (mut loaded, config) = league();
    with_roster_map(&mut loaded);
    loaded.traded_picks = vec![traded(1, 40, 20)];
    let mut first = pick(1, "q1", false);
    first.picked_by = Some("u4".into());

    let v = view(&loaded, &config);
    assert_eq!(v.draft.on_clock_slot, 3);
    loaded.api_picks = vec![first];
    let v = view(&loaded, &config);
    assert_eq!(v.rosters[3].players.len(), 1, "u4 is slot 4");
}

#[test]
fn third_round_reversal_moves_the_clock_and_my_picks() {
    let (mut loaded, config) = league();
    loaded.draft.settings.reversal_round = Some(3);

    let v = view(&loaded, &config);
    // 4 teams: round 1 is 1..4 forward, round 2 is 5..8 backward, and round 3
    // (9..12) repeats backward instead of turning — so slot 1 picks at 12 and
    // the snake carries on flipped from there.
    assert_eq!(v.draft.my_next_picks, vec![1, 8, 12, 13, 20, 21]);
    assert_eq!(v.draft.pick_slot_overrides.get(&9), Some(&4));
    assert_eq!(v.draft.pick_slot_overrides.get(&12), Some(&1));
    // Rounds 1 and 2 are an ordinary snake and carry no override.
    assert!(!v.draft.pick_slot_overrides.contains_key(&1));
    assert!(!v.draft.pick_slot_overrides.contains_key(&8));
}

#[test]
fn an_ordinary_snake_league_carries_no_overrides_at_all() {
    let (loaded, config) = league();
    let v = view(&loaded, &config);
    assert!(v.draft.pick_slot_overrides.is_empty());
    assert!(v.draft.keeper_picks.is_empty());
    assert_eq!(v.draft.my_next_picks, vec![1, 8, 9, 16, 17, 24]);
}

#[test]
fn an_auction_draft_is_modelled_as_a_snake_and_says_so() {
    let (mut loaded, config) = league();
    loaded.draft.draft_type = "auction".into();
    let v = view(&loaded, &config);
    assert!(
        v.data_health
            .warnings
            .iter()
            .any(|w| w.contains("auction") && w.contains("snake")),
        "{:?}",
        v.data_health.warnings
    );
}
