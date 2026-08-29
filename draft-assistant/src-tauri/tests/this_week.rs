//! The week ahead through `build_view`: the lineup check against the lineup
//! set on Sleeper and the matchup preview, from the draft's rosters plus the
//! week's `/matchups`.

use draft_assistant_lib::board::BoardPlayer;
use draft_assistant_lib::engine::{AppConfig, LoadedLeague};
use draft_assistant_lib::roster::RosterRules;
use draft_assistant_lib::sleeper::{Draft, League, Matchup, NflState, Pick};
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

fn board(id: &str, pos: &str, pts: f64) -> BoardPlayer {
    BoardPlayer {
        player_id: id.into(),
        name: id.into(),
        position: pos.into(),
        team: None,
        bye_week: None,
        points: pts,
        bonus_points: 0.0,
        vorp: 0.0,
        tier: 1,
        position_rank: 1,
        overall_rank: 1,
        adp: None,
        injury_status: None,
        sleeper_pts_ppr: None,
    }
}

/// 2 teams, slots WR/FLEX/BN, me at slot 2 (roster 20), them at slot 1
/// (roster 10). Draft complete: I hold a, b, c; they hold d, e, f.
fn league(matchups: Vec<Matchup>, state: Option<NflState>) -> (LoadedLeague, AppConfig) {
    let league: League = serde_json::from_value(serde_json::json!({
        "league_id": "l1", "name": "Week", "season": "2026", "status": "in_season",
        "total_rosters": 2, "roster_positions": ["WR", "FLEX", "BN"], "scoring_settings": {},
        "draft_id": "d1"
    }))
    .unwrap();
    let draft: Draft = serde_json::from_value(serde_json::json!({
        "draft_id": "d1", "status": "complete", "type": "snake", "season": "2026",
        "settings": {"teams": 2, "rounds": 3},
        "draft_order": {"me": 2, "them": 1},
        "slot_to_roster_id": {"1": 10, "2": 20}
    }))
    .unwrap();
    let board = vec![
        board("a", "WR", 200.0),
        board("b", "RB", 150.0),
        board("c", "WR", 120.0),
        board("d", "WR", 180.0),
        board("e", "RB", 160.0),
        board("f", "WR", 100.0),
    ];
    let board_index = board
        .iter()
        .enumerate()
        .map(|(i, p)| (p.player_id.clone(), i))
        .collect();
    // Week 4 rows: my b is the best this week, c is on a bye (no row).
    let weekly_points = HashMap::from([
        ("a".to_string(), vec![(4, 12.0)]),
        ("b".to_string(), vec![(4, 15.0)]),
        ("d".to_string(), vec![(4, 11.0)]),
        ("e".to_string(), vec![(4, 9.0)]),
        ("f".to_string(), vec![(4, 8.0)]),
    ]);
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
        roster_rules: RosterRules::new(&["WR".into(), "FLEX".into(), "BN".into()]),
        api_picks: vec![
            pick(1, 1, "them", "d"),
            pick(2, 2, "me", "a"),
            pick(3, 2, "me", "b"),
            pick(4, 1, "them", "e"),
            pick(5, 1, "them", "f"),
            pick(6, 2, "me", "c"),
        ],
        manual_picks: Vec::new(),
        traded_picks: Vec::new(),
        weekly_points,
        nfl_state: state,
        matchups,
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

fn matchup(roster_id: u32, matchup_id: u32, starters: &[&str]) -> Matchup {
    Matchup {
        roster_id,
        matchup_id: Some(matchup_id),
        starters: starters.iter().map(|s| s.to_string()).collect(),
        players: Vec::new(),
        points: 0.0,
        players_points: Default::default(),
    }
}

fn regular(week: u32) -> NflState {
    NflState {
        week,
        season_type: "regular".into(),
        season: Some("2026".into()),
    }
}

#[test]
fn the_week_comes_from_the_calendar_and_the_lineup_check_from_sleepers_starters() {
    // On Sleeper I have a in WR and c in FLEX — but c has no row this week
    // (bye) and b projects 15. FLEX should be b, +15.
    let (loaded, config) = league(
        vec![matchup(20, 1, &["a", "c"]), matchup(10, 1, &["d", "e"])],
        Some(regular(4)),
    );
    let view = build_view(&loaded, &config);
    let week = view.this_week.expect("a week ahead");
    assert_eq!(week.week, 4);
    let lineup = week.lineup.expect("lineup check");
    assert_eq!(lineup.set_points, 12.0, "c on a bye scores nothing");
    assert_eq!(lineup.best_points, 27.0);
    assert_eq!(lineup.changes.len(), 1);
    assert_eq!(lineup.changes[0].slot, "FLEX");
    assert_eq!(lineup.changes[0].in_.player_id, "b");
    assert_eq!(lineup.changes[0].gain, 15.0);
    // The standings use the same week.
    assert!(view.projected_standings.iter().all(|t| t.week == 4));
}

#[test]
fn the_matchup_names_the_opponent_and_scores_their_set_lineup() {
    let (loaded, config) = league(
        vec![matchup(20, 1, &["a", "b"]), matchup(10, 1, &["d", "f"])],
        Some(regular(4)),
    );
    let view = build_view(&loaded, &config);
    let m = view.this_week.unwrap().matchup.expect("a matchup");
    assert_eq!(m.opponent_slot, 1);
    assert_eq!(m.opponent_name.as_deref(), Some("Them"));
    assert_eq!(m.my_points, 27.0);
    // They set d + f (19), not their best d + e (20).
    assert_eq!(m.opponent_points, 19.0);
    assert!(m.win_probability > 0.5);
}

#[test]
fn before_the_season_the_opener_is_planned_for_and_no_matchups_means_no_week() {
    let (loaded, config) = league(Vec::new(), None);
    let view = build_view(&loaded, &config);
    assert!(view.this_week.is_none(), "nothing to say without matchups");
    assert!(view.projected_standings.iter().all(|t| t.week == 1));
}

/// Draft night's own failure: my roster has no receiver for the dedicated WR
/// slot and I am down to my last pick.
#[test]
fn an_empty_required_slot_with_the_last_pick_coming_is_called_out() {
    let (mut loaded, config) = league(Vec::new(), None);
    loaded.draft.status = "drafting".into();
    // Picks 1–5 made; only pick 6 (mine) remains. I hold b and c — an RB
    // and a WR — so the WR slot is filled. Swap c for their receiver to
    // leave my WR slot empty: I hold two backs.
    loaded.api_picks = vec![
        pick(1, 1, "them", "d"),
        pick(2, 2, "me", "b"),
        pick(3, 2, "me", "e"),
        pick(4, 1, "them", "c"),
        pick(5, 1, "them", "f"),
    ];
    let view = build_view(&loaded, &config);
    assert_eq!(view.draft.my_next_picks, vec![6]);
    assert_eq!(
        view.draft.starter_alert.as_deref(),
        Some("WR still empty with 1 pick left")
    );
    // Once the draft is over there is nothing to shout about.
    loaded.draft.status = "complete".into();
    loaded.api_picks.push(pick(6, 2, "me", "a"));
    let view = build_view(&loaded, &config);
    assert_eq!(view.draft.starter_alert, None);
}
