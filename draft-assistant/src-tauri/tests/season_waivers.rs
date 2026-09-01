//! The waiver pool is cut down before it is evaluated. This checks the cut is
//! made on the projection the gain is measured against — a free agent with a
//! poor season rank but a big week must still be seen.

use draft_assistant_lib::board::BoardPlayer;
use draft_assistant_lib::engine::{AppConfig, LoadedLeague};
use draft_assistant_lib::roster::RosterRules;
use draft_assistant_lib::season::build_season_view;
use draft_assistant_lib::season_api::Roster;
use draft_assistant_lib::season_engine::LoadedSeason;
use draft_assistant_lib::season_history::History;
use draft_assistant_lib::sleeper::{Draft, League, ProjectionRow};
use draft_assistant_lib::valuation::ReplacementModel;
use draft_assistant_lib::weekly::WeeklyPoints;
use std::collections::HashMap;

/// Free agents ranked ahead of the streamer on the season board. Comfortably
/// more than the pool the evaluator keeps, so the streamer starts outside it.
const FILLERS: usize = 120;
const WEEK: u32 = 2;

fn league() -> League {
    serde_json::from_str(
        r#"{
            "league_id": "league-waivers",
            "name": "Waiver League",
            "season": "2025",
            "status": "in_season",
            "total_rosters": 1,
            "roster_positions": ["RB", "BN"],
            "scoring_settings": {"rec": 1.0},
            "draft_id": "draft-waivers",
            "settings": {"playoff_week_start": 15, "waiver_budget": 100}
        }"#,
    )
    .unwrap()
}

fn draft() -> Draft {
    serde_json::from_str(
        r#"{
            "draft_id": "draft-waivers",
            "status": "complete",
            "type": "snake",
            "settings": {"teams": 1, "rounds": 2}
        }"#,
    )
    .unwrap()
}

fn board_player(id: &str, rank: u32) -> BoardPlayer {
    BoardPlayer {
        player_id: id.to_string(),
        name: id.to_uppercase(),
        position: "RB".into(),
        team: Some("LV".into()),
        bye_week: None,
        points: 300.0 - rank as f64,
        bonus_points: 0.0,
        vorp: 100.0 - rank as f64,
        tier: 1,
        position_rank: rank,
        overall_rank: rank,
        adp: Some(rank as f64),
        injury_status: None,
        sleeper_pts_ppr: None,
    }
}

fn week_row(id: &str, points: f64) -> ProjectionRow {
    ProjectionRow {
        player_id: id.to_string(),
        stats: Some(HashMap::from([("rec".to_string(), points)])),
        week: Some(WEEK),
        player: None,
        opponent: None,
    }
}

/// One team starting a mediocre back, plus a board of free agents: `FILLERS`
/// of them ranked ahead of "streamer", who is dead last on season rank but
/// projects for far more points this week than anyone else available.
fn loaded_league() -> LoadedLeague {
    let league = league();
    let scoring = league.scoring_settings.clone();
    let roster_rules = RosterRules::new(&league.roster_positions);

    let mut board = vec![board_player("mine", 1)];
    let mut rows = vec![week_row("mine", 8.0)];
    for i in 0..FILLERS {
        let id = format!("filler{i}");
        board.push(board_player(&id, i as u32 + 2));
        // Every filler is worse than the back I already start, so none of them
        // can change my lineup.
        rows.push(week_row(&id, 2.0));
    }
    board.push(board_player("streamer", FILLERS as u32 + 2));
    rows.push(week_row("streamer", 25.0));

    let board_index = board
        .iter()
        .enumerate()
        .map(|(i, p)| (p.player_id.clone(), i))
        .collect();
    LoadedLeague {
        league,
        draft: draft(),
        user_names: HashMap::from([("user-1".to_string(), "Me".to_string())]),
        user_avatars: HashMap::new(),
        board,
        board_index,
        replacement_model: ReplacementModel {
            demand: HashMap::new(),
            baseline: HashMap::new(),
        },
        roster_rules,
        api_picks: Vec::new(),
        manual_picks: Vec::new(),
        traded_picks: Vec::new(),
        keeper_pick_nos: Default::default(),
        poll_last_success_at: None,
        poll_consecutive_failures: 0,
        poll_last_error: None,
        players_fetched_at: 0,
        projections_fetched_at: 0,
        weekly_fetched_at: 0,
        warnings: Vec::new(),
        player_meta: HashMap::new(),
        weekly_points: WeeklyPoints::build(&rows, &scoring),
    }
}

fn season() -> LoadedSeason {
    let roster: Roster = serde_json::from_value(serde_json::json!({
        "roster_id": 1,
        "owner_id": "user-1",
        "players": ["mine"],
        "starters": ["mine"],
    }))
    .unwrap();
    LoadedSeason {
        week: WEEK,
        season: 2025,
        rosters: vec![roster],
        matchups: Vec::new(),
        schedule: Vec::new(),
        season_points: HashMap::new(),
        transactions: Vec::new(),
        scores: Vec::new(),
        last_season: Vec::new(),
        history: History::default(),
        fetched_at: 0,
        warnings: Vec::new(),
        sources: Default::default(),
    }
}

#[test]
fn a_low_ranked_free_agent_with_a_big_week_is_still_a_waiver_target() {
    let config = AppConfig {
        my_user_id: Some("user-1".into()),
        ..AppConfig::default()
    };
    let view = build_season_view(&loaded_league(), &season(), config.my_user_id.as_deref());
    assert!(
        view.waivers.iter().any(|w| w.player_id == "streamer"),
        "the best week-2 free agent was never evaluated: {:?}",
        view.waivers
            .iter()
            .map(|w| &w.player_id)
            .collect::<Vec<_>>()
    );
    let top = &view.waivers[0];
    assert_eq!(top.player_id, "streamer");
    assert!(
        (top.gain_points - 17.0).abs() < 1e-9,
        "25 for the streamer minus the 8 he displaces, got {}",
        top.gain_points
    );
}
