//! Leagues that trade draft picks. Written after the 2026 draft: 40 picks had
//! changed hands, 36 of the first 150 were made by someone other than the
//! slot's owner, and the app — reading the snake alone — had nine of the
//! thirteen opponent rosters wrong and named the wrong manager on the clock.

use draft_assistant_lib::board::BoardPlayer;
use draft_assistant_lib::engine::{AppConfig, LoadedLeague};
use draft_assistant_lib::roster::RosterRules;
use draft_assistant_lib::sleeper::{Draft, League, Pick, TradedPick};
use draft_assistant_lib::valuation::ReplacementModel;
use draft_assistant_lib::view::build_view;
use std::collections::HashMap;

fn pick(pick_no: u32, slot: u32, picked_by: &str, player_id: &str) -> Pick {
    Pick {
        round: pick_no.div_ceil(2),
        pick_no,
        draft_slot: slot,
        player_id: player_id.into(),
        picked_by: Some(picked_by.into()),
        metadata: None,
        is_keeper: None,
    }
}

/// 2 teams × 3 rounds, me at slot 2 (roster 20), them at slot 1 (roster 10).
/// Their round-2 pick — pick 4 in a snake — has been traded to me.
fn league_with(api_picks: Vec<Pick>, traded: Vec<TradedPick>) -> (LoadedLeague, AppConfig) {
    let league: League = serde_json::from_value(serde_json::json!({
        "league_id": "l1", "name": "Traders", "season": "2026", "status": "drafting",
        "total_rosters": 2, "roster_positions": ["WR", "BN"], "scoring_settings": {},
        "draft_id": "d1"
    }))
    .unwrap();
    let draft: Draft = serde_json::from_value(serde_json::json!({
        "draft_id": "d1", "status": "drafting", "type": "snake", "season": "2026",
        "settings": {"teams": 2, "rounds": 3},
        "draft_order": {"me": 2, "them": 1},
        "slot_to_roster_id": {"1": 10, "2": 20}
    }))
    .unwrap();
    let board: Vec<BoardPlayer> = ["a", "b", "c", "d", "e", "f"]
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
        user_names: HashMap::from([
            ("me".to_string(), "Me".to_string()),
            ("them".to_string(), "Them".to_string()),
        ]),
        board,
        board_index,
        replacement_model: ReplacementModel::default(),
        roster_rules: RosterRules::new(&["WR".into(), "BN".into()]),
        api_picks,
        manual_picks: Vec::new(),
        traded_picks: traded,
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

fn their_round_two_is_mine() -> Vec<TradedPick> {
    vec![TradedPick {
        season: "2026".into(),
        round: 2,
        roster_id: 10,
        owner_id: 20,
        previous_owner_id: Some(10),
    }]
}

#[test]
fn a_pick_i_acquired_is_one_of_my_next_picks_and_puts_me_on_the_clock() {
    // Picks 1–3 made; pick 4 is up. In the snake it is slot 1's — but I own it.
    let picks = vec![
        pick(1, 1, "them", "a"),
        pick(2, 2, "me", "b"),
        pick(3, 2, "me", "c"),
    ];
    let (loaded, config) = league_with(picks, their_round_two_is_mine());
    let view = build_view(&loaded, &config);

    assert_eq!(view.draft.current_pick, 4);
    assert_eq!(
        view.draft.on_clock_slot, 2,
        "the acquired pick is on my clock"
    );
    assert_eq!(view.draft.on_clock_name.as_deref(), Some("Me"));
    assert!(view.draft.is_my_pick);
    assert_eq!(view.draft.my_next_picks, vec![4, 6]);
    assert_eq!(view.draft.traded_pick_slots, HashMap::from([(4, 2)]));
}

#[test]
fn a_pick_made_from_a_traded_slot_lands_on_the_roster_that_made_it() {
    // They used my pick 3? No — I used *their* pick 4, from slot 1.
    let picks = vec![
        pick(1, 1, "them", "a"),
        pick(2, 2, "me", "b"),
        pick(3, 2, "me", "c"),
        pick(4, 1, "me", "d"),
    ];
    let (loaded, config) = league_with(picks, their_round_two_is_mine());
    let view = build_view(&loaded, &config);

    let mine = view.my_roster.expect("my roster");
    let names: Vec<&str> = mine.players.iter().map(|p| p.name.as_str()).collect();
    assert_eq!(
        names,
        vec!["b", "c", "d"],
        "d was picked from slot 1 but by me"
    );
    let theirs = &view.rosters[0];
    assert_eq!(
        theirs.players.len(),
        1,
        "slot 1 keeps only the pick it made"
    );
    // And the recent-picks list credits the manager, not the slot.
    let last = view.recent_picks.first().expect("a recent pick");
    assert_eq!((last.pick_no, last.slot), (4, 2));
    assert_eq!(last.slot_name.as_deref(), Some("Me"));
}

#[test]
fn without_trades_nothing_changes() {
    let picks = vec![pick(1, 1, "them", "a"), pick(2, 2, "me", "b")];
    let (loaded, config) = league_with(picks, Vec::new());
    let view = build_view(&loaded, &config);
    assert_eq!(view.draft.on_clock_slot, 2, "pick 3 is slot 2's in a snake");
    assert_eq!(view.draft.my_next_picks, vec![3, 6]);
    assert!(view.draft.traded_pick_slots.is_empty());
}

/// `picked_by` is missing on a manual pick: it falls to whoever owns the
/// pick number — the acquirer, not the slot the snake would name.
#[test]
fn a_manual_pick_with_no_user_goes_to_the_owner_of_the_pick_number() {
    let mut manual = pick(4, 1, "me", "d");
    manual.picked_by = None;
    let picks = vec![
        pick(1, 1, "them", "a"),
        pick(2, 2, "me", "b"),
        pick(3, 2, "me", "c"),
        manual,
    ];
    let (loaded, config) = league_with(picks, their_round_two_is_mine());
    let view = build_view(&loaded, &config);
    let mine = view.my_roster.expect("my roster");
    assert!(mine.players.iter().any(|p| p.name == "d"));
}
