//! Strength snapshots: measuring rosters and persisting history to disk.

use draft_assistant_lib::engine::{Engine, LoadedLeague};
use draft_assistant_lib::roster::RosterRules;
use draft_assistant_lib::season_api::Roster;
use draft_assistant_lib::season_engine::LoadedSeason;
use draft_assistant_lib::season_history::HistoryStore;
use draft_assistant_lib::season_history::{take_snapshot, History};
use draft_assistant_lib::sleeper::{Draft, League, PlayerMeta, ProjectionRow};
use draft_assistant_lib::valuation::ReplacementModel;
use draft_assistant_lib::weekly::WeeklyPoints;
use std::collections::HashMap;

/// A league whose regular season is weeks 1 and 2 (playoffs start week 3).
fn league() -> League {
    serde_json::from_str(
        r#"{
            "league_id": "league-hist",
            "name": "History League",
            "season": "2025",
            "status": "in_season",
            "total_rosters": 2,
            "roster_positions": ["QB", "BN"],
            "scoring_settings": {"rec": 1.0},
            "settings": {"playoff_week_start": 3}
        }"#,
    )
    .unwrap()
}

fn draft() -> Draft {
    serde_json::from_str(
        r#"{
            "draft_id": "draft-hist",
            "status": "complete",
            "type": "snake",
            "settings": {"teams": 2, "rounds": 2}
        }"#,
    )
    .unwrap()
}

fn weekly_rows() -> Vec<ProjectionRow> {
    serde_json::from_str(
        r#"[
            {"player_id": "qb1", "stats": {"rec": 10.0}, "week": 1},
            {"player_id": "qb1", "stats": {"rec": 4.0}, "week": 2},
            {"player_id": "wr1", "stats": {"rec": 5.0}, "week": 1},
            {"player_id": "wr1", "stats": {"rec": 5.0}, "week": 2}
        ]"#,
    )
    .unwrap()
}

fn player_meta() -> HashMap<String, PlayerMeta> {
    serde_json::from_str(
        r#"{
            "qb1": {"full_name": "Q. Back", "position": "QB", "injury_status": "Questionable"},
            "wr1": {"full_name": "W. Receiver", "position": "WR"}
        }"#,
    )
    .unwrap()
}

fn loaded_league() -> LoadedLeague {
    let league = league();
    let roster_rules = RosterRules::new(&league.roster_positions);
    let scoring = league.scoring_settings.clone();
    LoadedLeague {
        league,
        draft: draft(),
        user_names: HashMap::new(),
        user_avatars: HashMap::new(),
        board: Vec::new(),
        board_index: HashMap::new(),
        replacement_model: ReplacementModel {
            demand: HashMap::new(),
            baseline: HashMap::new(),
        },
        roster_rules,
        api_picks: Vec::new(),
        manual_picks: Vec::new(),
        poll_last_success_at: None,
        poll_consecutive_failures: 0,
        poll_last_error: None,
        players_fetched_at: 0,
        projections_fetched_at: 0,
        weekly_fetched_at: 0,
        warnings: Vec::new(),
        player_meta: player_meta(),
        weekly_points: WeeklyPoints::build(&weekly_rows(), &scoring),
    }
}

fn roster(players: &[&str]) -> Roster {
    serde_json::from_value(serde_json::json!({
        "roster_id": 1,
        "owner_id": "user-1",
        "players": players,
    }))
    .unwrap()
}

fn season(week: u32, rosters: Vec<Roster>, fetched_at: u64) -> LoadedSeason {
    LoadedSeason {
        week,
        season: 2025,
        rosters,
        matchups: Vec::new(),
        schedule: Vec::new(),
        season_points: HashMap::new(),
        transactions: Vec::new(),
        scores: Vec::new(),
        last_season: Vec::new(),
        history: History::default(),
        fetched_at,
        warnings: Vec::new(),
        sources: Default::default(),
    }
}

#[test]
fn snapshot_measures_best_lineup_strength_per_remaining_week() {
    let loaded = loaded_league();
    let season = season(1, vec![roster(&["qb1", "wr1"])], 0);

    let snap = take_snapshot(&loaded, &season, 777);
    assert_eq!(snap.taken_at, 777);
    assert_eq!(snap.week, 1);
    assert_eq!(snap.teams.len(), 1);
    let team = &snap.teams[0];
    assert_eq!(team.roster_id, 1);
    // Only the QB slot starts; wr1 rides the bench. (10 + 4) / 2 weeks.
    assert!(
        (team.strength - 7.0).abs() < 1e-9,
        "strength {}",
        team.strength
    );
    let qb = &team.players["qb1"];
    assert!((qb.points - 7.0).abs() < 1e-9, "qb mean {}", qb.points);
    assert_eq!(qb.injury.as_deref(), Some("Questionable"));
    let wr = &team.players["wr1"];
    assert!((wr.points - 5.0).abs() < 1e-9, "wr mean {}", wr.points);
    assert!(wr.injury.is_none());
}

#[test]
fn snapshot_in_the_final_week_uses_that_week_alone() {
    let loaded = loaded_league();
    let season = season(2, vec![roster(&["qb1"])], 0);
    let snap = take_snapshot(&loaded, &season, 1);
    // Week 2 is the last regular week: strength is week 2's lineup only.
    assert!((snap.teams[0].strength - 4.0).abs() < 1e-9);
}

#[test]
fn record_history_persists_once_and_skips_quiet_refreshes() {
    let dir = std::env::temp_dir().join(format!(
        "draft-assistant-history-{}-{}",
        std::process::id(),
        std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .unwrap()
            .as_nanos()
    ));
    let engine = Engine::new(dir.clone());
    let loaded = loaded_league();
    let season = season(1, vec![roster(&["qb1", "wr1"])], 0);

    let first = engine.record_history(&loaded, &season);
    assert_eq!(first.snapshots.len(), 1);
    assert!(dir.join("history_league-hist.json").is_file());

    let unchanged = engine.record_history(&loaded, &season);
    assert_eq!(
        unchanged.snapshots.len(),
        1,
        "same rosters inside the quiet window add nothing"
    );

    let traded = season_with_new_player();
    let after_trade = engine.record_history(&loaded, &traded);
    assert_eq!(
        after_trade.snapshots.len(),
        2,
        "a roster change forces a snapshot immediately"
    );

    std::fs::remove_dir_all(dir).unwrap();
}

fn season_with_new_player() -> LoadedSeason {
    season(1, vec![roster(&["qb1"])], 0)
}

#[test]
fn live_slice_staleness_is_measured_from_fetch_time() {
    let now = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .unwrap()
        .as_secs();
    let fresh = season(1, Vec::new(), now);
    assert!(Engine::live_age(&fresh) < 5);
    assert!(!Engine::live_is_stale(&fresh));

    let stale = season(1, Vec::new(), now.saturating_sub(3600));
    assert!(Engine::live_age(&stale) >= 3600);
    assert!(Engine::live_is_stale(&stale));
}
