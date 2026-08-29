//! Keeper leagues: picks that are already filled in before the draft starts,
//! at scattered pick numbers all over the board.
//!
//! Written after the 2026 league turned out to carry 23 keepers at picks 11,
//! 14, 20 … 177 while the draft was still `pre_draft`: the app counted them as
//! "23 picks made" and put the draft on pick 24 before anybody had picked.

use draft_assistant_lib::board::BoardPlayer;
use draft_assistant_lib::engine::{AppConfig, LoadedLeague};
use draft_assistant_lib::roster::RosterRules;
use draft_assistant_lib::sleeper::{Draft, League, Pick};
use draft_assistant_lib::valuation::ReplacementModel;
use draft_assistant_lib::view::{build_view, merged_picks};
use std::collections::HashMap;

fn keeper(pick_no: u32, slot: u32, player_id: &str) -> Pick {
    Pick {
        round: pick_no.div_ceil(2),
        pick_no,
        draft_slot: slot,
        player_id: player_id.into(),
        picked_by: None,
        metadata: None,
        is_keeper: None,
    }
}

/// 2 teams × 3 rounds, my user at slot 2 (picks 2, 3, 6 in a snake).
fn league_with(api_picks: Vec<Pick>) -> (LoadedLeague, AppConfig) {
    let league: League = serde_json::from_value(serde_json::json!({
        "league_id": "l1", "name": "Keepers", "season": "2026", "status": "pre_draft",
        "total_rosters": 2, "roster_positions": ["WR", "BN"], "scoring_settings": {},
        "draft_id": "d1"
    }))
    .unwrap();
    let draft: Draft = serde_json::from_value(serde_json::json!({
        "draft_id": "d1", "status": "pre_draft", "type": "snake",
        "settings": {"teams": 2, "rounds": 3},
        "draft_order": {"me": 2, "them": 1}
    }))
    .unwrap();
    let board: Vec<BoardPlayer> = ["a", "b", "c", "d", "e", "f", "g"]
        .iter()
        .map(|id| BoardPlayer {
            player_id: (*id).into(),
            name: (*id).into(),
            position: "WR".into(),
            team: None,
            bye_week: None,
            points: 100.0,
            bonus_points: 0.0,
            vorp: 10.0,
            tier: 1,
            position_rank: 1,
            overall_rank: 1,
            adp: None,
            injury_status: None,
            sleeper_pts_ppr: None,
        })
        .collect();
    let board_index = board
        .iter()
        .enumerate()
        .map(|(i, p)| (p.player_id.clone(), i))
        .collect();
    let loaded = LoadedLeague {
        league,
        draft,
        user_names: HashMap::from([("me".to_string(), "mcsleeper26".to_string())]),
        board,
        board_index,
        replacement_model: ReplacementModel::default(),
        roster_rules: RosterRules::new(&["WR".into(), "BN".into()]),
        api_picks,
        manual_picks: Vec::new(),
        traded_picks: Vec::new(),
        weekly_points: HashMap::new(),
        nfl_state: None,
        matchups: Vec::new(),
        trending: Vec::new(),
        league_rosters: Vec::new(),
        past_matchups: Vec::new(),
        transactions: Vec::new(),
        schedule: Vec::new(),
        history: None,
        keeper_pick_nos: Default::default(),
        poll_last_success_at: None,
        poll_consecutive_failures: 0,
        poll_last_error: None,
        players_fetched_at: 0,
        projections_fetched_at: 0,
        weekly_fetched_at: 0,
        warnings: Vec::new(),
        player_meta: HashMap::new(),
    };
    let config = AppConfig {
        my_user_id: Some("me".into()),
        active_league_id: Some("l1".into()),
        leagues: Vec::new(),
    };
    (loaded, config)
}

#[test]
fn a_keeper_late_on_the_board_does_not_advance_the_clock() {
    let (loaded, config) = league_with(vec![keeper(5, 1, "e")]);
    let view = build_view(&loaded, &config);

    assert_eq!(view.draft.current_pick, 1, "the draft starts at pick 1");
    assert_eq!(view.draft.current_round, 1);
    assert_eq!(view.draft.total_picks_made, 1);
    assert!(!view.draft.is_my_pick, "slot 1 opens a snake");
}

#[test]
fn my_next_picks_leave_out_the_ones_a_keeper_already_used() {
    // My slot's picks are 2, 3 and 6; a keeper of mine occupies pick 3.
    let (loaded, config) = league_with(vec![keeper(3, 2, "c")]);
    let view = build_view(&loaded, &config);

    assert_eq!(view.draft.current_pick, 1);
    assert_eq!(view.draft.my_next_picks, vec![2, 6]);
    // Only pick 1 has to happen before mine — pick 3 is already in the book.
    assert_eq!(view.draft.picks_until_mine, Some(1));
}

#[test]
fn the_clock_skips_over_a_keeper_when_it_reaches_one() {
    // Picks 1 and 2 are done and pick 3 is a keeper: the draft is on pick 4.
    let (loaded, config) = league_with(vec![
        keeper(1, 1, "a"),
        keeper(2, 2, "b"),
        keeper(3, 2, "c"),
    ]);
    let view = build_view(&loaded, &config);

    assert_eq!(view.draft.current_pick, 4);
    assert_eq!(view.draft.current_round, 2);
    assert_eq!(view.draft.on_clock_slot, 1);
}

#[test]
fn a_manual_pick_is_not_swallowed_by_a_later_keeper() {
    // The fallback used to keep only manual picks numbered above every API
    // pick, so with a keeper at 177 nothing a user marked would ever show.
    let api = vec![keeper(177, 1, "z")];
    let manual = vec![keeper(1, 1, "a")];
    let merged = merged_picks(&api, &manual);
    assert_eq!(
        merged.iter().map(|p| p.pick_no).collect::<Vec<_>>(),
        vec![1, 177]
    );
}

#[test]
fn a_manual_pick_takes_the_next_open_number_not_the_pick_count() {
    let (mut loaded, config) = league_with(vec![keeper(5, 1, "e")]);
    draft_assistant_lib::manual::apply_manual_pick(&mut loaded, "a".into()).unwrap();

    assert_eq!(loaded.manual_picks[0].pick_no, 1);
    assert_eq!(loaded.manual_picks[0].round, 1);
    let view = build_view(&loaded, &config);
    assert_eq!(view.draft.current_pick, 2);
    assert_eq!(view.draft.total_picks_made, 2);
}

#[test]
fn recent_picks_and_runs_ignore_keepers_the_draft_has_not_reached() {
    // Pick 1 has been made; picks 5 and 6 are keepers sitting in round 3.
    let (loaded, config) = league_with(vec![
        keeper(1, 1, "a"),
        keeper(5, 1, "e"),
        keeper(6, 2, "f"),
    ]);
    let view = build_view(&loaded, &config);

    assert_eq!(view.draft.current_pick, 2);
    let shown: Vec<u32> = view.recent_picks.iter().map(|p| p.pick_no).collect();
    assert_eq!(
        shown,
        vec![1],
        "keepers ahead of the clock are not recent picks"
    );
}

#[test]
fn keepers_are_tagged_on_rosters_whether_or_not_sleeper_flags_them() {
    // The 2026 feed carried 27 keepers and flagged 24: position, not the
    // flag, decides. Pick 1 is open, so anything already in the book is kept.
    let mut flagged = keeper(5, 1, "e");
    flagged.is_keeper = Some(true);
    let (loaded, config) = league_with(vec![flagged, keeper(6, 2, "f")]);
    let view = build_view(&loaded, &config);
    let mine = view.my_roster.as_ref().unwrap();
    assert_eq!(mine.players.len(), 1);
    assert!(mine.players[0].is_keeper, "unflagged keeper at pick 6");
    assert!(
        view.rosters[0].players[0].is_keeper,
        "flagged keeper at pick 5"
    );
    assert!(
        view.recent_picks.is_empty(),
        "keepers are not picks made tonight"
    );
}

#[test]
fn a_keeper_stays_a_keeper_after_the_draft_passes_its_slot() {
    // Picks 1-3 have been made and the unflagged keeper sat at 3 all along;
    // the league remembers it as one, so it is neither "recent" nor drafted.
    let (mut loaded, config) = league_with(vec![
        keeper(1, 1, "a"),
        keeper(2, 2, "b"),
        keeper(3, 2, "c"),
    ]);
    loaded.keeper_pick_nos.insert(3);
    let view = build_view(&loaded, &config);
    assert_eq!(view.draft.current_pick, 4);
    let mine = view.my_roster.as_ref().unwrap();
    let kept: Vec<(u32, bool)> = mine
        .players
        .iter()
        .map(|p| (p.pick_no, p.is_keeper))
        .collect();
    assert_eq!(kept, vec![(2, false), (3, true)]);
    let recent: Vec<u32> = view.recent_picks.iter().map(|p| p.pick_no).collect();
    assert_eq!(recent, vec![2, 1], "newest first, keeper left out");

    // Without the memory the same feed reads as three ordinary picks.
    loaded.keeper_pick_nos.clear();
    let view = build_view(&loaded, &config);
    assert!(view.my_roster.unwrap().players.iter().all(|p| !p.is_keeper));
    assert_eq!(view.recent_picks.len(), 3);
}
